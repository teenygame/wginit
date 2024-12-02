//! wginit is a simple framework for initializing wgpu + winit.
//!
//! It only handles one device.

pub use wgpu;
pub use winit;

/// A context struct passed to application handlers while the application is not suspended.
///
/// It contains all wgpu and winit state.
pub struct Context<'a> {
    /// The current [`winit::event_loop::ActiveEventLoop`].
    ///
    /// <section class="warning">
    ///
    /// You should **not** call [`winit::event_loop::ActiveEventLoop::create_window`] as wginit does not support multiple windows.
    ///
    /// </section>
    pub event_loop: &'a winit::event_loop::ActiveEventLoop,

    /// The current [`winit::window::Window`]. This may be [`None`] if the window is not available yet.
    pub window: Option<&'a winit::window::Window>,

    /// The current wgpu state. This may be [`None`] if the wgpu state is not available yet, or was destroyed.
    pub wgpu: Option<&'a Wgpu>,
}

impl<'a> Context<'a> {
    fn new(
        event_loop: &'a winit::event_loop::ActiveEventLoop,
        window: Option<&'a winit::window::Window>,
        wgpu: Option<&'a Wgpu>,
    ) -> Self {
        Self {
            event_loop,
            window,
            wgpu,
        }
    }
}

/// The current wgpu state.
pub struct Wgpu {
    /// The current [`wgpu::Device`].
    pub device: wgpu::Device,
    /// The current [`wgpu::Queue`].
    pub queue: wgpu::Queue,
    /// The current [`wgpu::Adapter`].
    pub adapter: wgpu::Adapter,
    /// The current [`wgpu::Surface`].
    pub surface: wgpu::Surface<'static>,
    /// The current counter for times the wgpu state has been suspended.
    ///
    /// This can be useful to determine if the wgpu state was reinitialized from the last time the wgpu state was passed.
    pub suspend_count: u64,
}

impl Wgpu {
    async fn new<A>(window: std::sync::Arc<winit::window::Window>, suspend_count: u64) -> Self
    where
        A: ApplicationHandler,
    {
        let instance = new_wgpu_instance().await;

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&A::request_adapter_options(&surface))
            .await
            .expect("failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(&A::device_descriptor(&adapter), None)
            .await
            .expect("failed to create device");

        surface.configure(
            &device,
            &A::surface_configuration(&surface, &adapter, window.inner_size()),
        );

        Self {
            device,
            queue,
            adapter,
            surface,
            suspend_count,
        }
    }
}

async fn new_wgpu_instance() -> wgpu::Instance {
    // Taken from https://github.com/emilk/egui/blob/454abf705b87aba70cef582d6ce80f74aa398906/crates/eframe/src/web/web_painter_wgpu.rs#L117-L166
    //
    // We try to see if we can use default backends first to initialize an adapter. If not, we fall back on GL.
    let instance = wgpu::Instance::default();

    if instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            ..Default::default()
        })
        .await
        .is_none()
    {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..Default::default()
        })
    } else {
        instance
    }
}

enum UserEvent<C> {
    WgpuReady(Wgpu),
    Custom(C),
}

/// Sender for user events.
#[derive(Clone)]
pub struct UserEventSender<C>(winit::event_loop::EventLoopProxy<UserEvent<C>>)
where
    C: 'static;

impl<C> UserEventSender<C>
where
    C: 'static,
{
    /// Sends a user event to the application.
    pub fn send_event(&self, event: C) -> Result<(), winit::event_loop::EventLoopClosed<C>> {
        self.0.send_event(UserEvent::Custom(event)).map_err(|e| {
            let UserEvent::Custom(e) = e.0 else {
                unreachable!()
            };
            winit::event_loop::EventLoopClosed(e)
        })
    }
}

struct WinitApplicationHandler<A>
where
    A: ApplicationHandler,
{
    app: A,
    window: Option<std::sync::Arc<winit::window::Window>>,
    wgpu: Option<Wgpu>,
    suspend_count: u64,
    event_loop_proxy: winit::event_loop::EventLoopProxy<UserEvent<A::UserEvent>>,
}

impl<A> WinitApplicationHandler<A>
where
    A: ApplicationHandler,
{
    fn new(app: A, event_loop: &winit::event_loop::EventLoop<UserEvent<A::UserEvent>>) -> Self {
        Self {
            app,
            window: None,
            wgpu: None,
            suspend_count: 0,
            event_loop_proxy: event_loop.create_proxy(),
        }
    }
}

impl<A> winit::application::ApplicationHandler<UserEvent<A::UserEvent>>
    for WinitApplicationHandler<A>
where
    A: ApplicationHandler,
{
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = self
            .window
            .get_or_insert_with(|| {
                std::sync::Arc::new(
                    event_loop
                        .create_window(A::window_attrs())
                        .expect("failed to create window"),
                )
            })
            .clone();

        let event_loop_proxy = self.event_loop_proxy.clone();
        let fut = async move {
            assert!(event_loop_proxy
                .send_event(UserEvent::WgpuReady(
                    Wgpu::new::<A>(window, self.suspend_count).await
                ))
                .is_ok());
        };

        #[cfg(not(target_arch = "wasm32"))]
        pollster::block_on(fut);

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(fut);
    }

    fn suspended(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.wgpu = None;
        self.suspend_count += 1;
        self.app.suspended(&Context::new(
            event_loop,
            self.window.as_ref().map(|window| window.as_ref()),
            self.wgpu.as_ref(),
        ));
    }

    fn exiting(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.app.exiting(&Context::new(
            event_loop,
            self.window.as_ref().map(|window| window.as_ref()),
            self.wgpu.as_ref(),
        ));
    }

    fn memory_warning(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.app.memory_warning(&Context::new(
            event_loop,
            self.window.as_ref().map(|window| window.as_ref()),
            self.wgpu.as_ref(),
        ));
    }

    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        start_cause: winit::event::StartCause,
    ) {
        self.app.new_events(event_loop, start_cause);
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.app.about_to_wait(&Context::new(
            event_loop,
            self.window.as_ref().map(|window| window.as_ref()),
            self.wgpu.as_ref(),
        ));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::Resized(size) => {
                let window = self.window.as_ref().unwrap();
                let Some(wgpu) = self.wgpu.as_ref() else {
                    return;
                };
                wgpu.surface.configure(
                    &wgpu.device,
                    &A::surface_configuration(&wgpu.surface, &wgpu.adapter, size),
                );
                window.request_redraw();
            }
            winit::event::WindowEvent::RedrawRequested => {
                let window = self.window.as_ref().unwrap();
                let Some(wgpu) = self.wgpu.as_ref() else {
                    return;
                };
                self.app.redraw(window, wgpu);
            }
            _ => {}
        };

        self.app.window_event(
            &Context::new(
                event_loop,
                self.window.as_ref().map(|window| window.as_ref()),
                self.wgpu.as_ref(),
            ),
            event,
        );
    }

    fn user_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserEvent<A::UserEvent>,
    ) {
        match event {
            UserEvent::WgpuReady(wgpu) => {
                // We can just unwrap here because if we're getting the wgpu state we can safely assume the window is already initialized, otherwise we have bigger problems.
                let window = self.window.as_ref().unwrap();
                self.wgpu = Some(wgpu);
                self.app.resumed(&Context::new(
                    event_loop,
                    Some(window.as_ref()),
                    self.wgpu.as_ref(),
                ));
                window.request_redraw();
            }
            UserEvent::Custom(e) => {
                self.app.user_event(
                    &Context::new(
                        event_loop,
                        self.window.as_ref().map(|window| window.as_ref()),
                        self.wgpu.as_ref(),
                    ),
                    e,
                );
            }
        }
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        self.app.device_event(
            &Context::new(
                event_loop,
                self.window.as_ref().map(|window| window.as_ref()),
                self.wgpu.as_ref(),
            ),
            device_id,
            event,
        );
    }
}

/// The application event handler.
///
/// Most of these methods reflect those in [`winit::application::ApplicationHandler`].
///
/// Additionally, events can be delivered to the event loop via the [`UserEventSender`] passed to [`ApplicationHandler::new`]. If used, they can be handled via [`ApplicationHandler::user_event`].
pub trait ApplicationHandler
where
    Self::UserEvent: 'static,
{
    /// The type of user event for this application.
    ///
    /// If no user events are desired, you can use [`std::convert::Infallible`] for the type.
    type UserEvent;

    /// Gets the window attributes for creating a window for this application.
    fn window_attrs() -> winit::window::WindowAttributes {
        #[allow(unused_mut)]
        let mut window_attrs = winit::window::WindowAttributes::default();
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys as _;
            window_attrs = window_attrs.with_append(true);
        }
        window_attrs
    }

    /// Creates the [`wgpu::DeviceDescriptor`] to create a [`wgpu::Device`] with.
    ///
    /// The defaults are compatible with WebGL.
    fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor {
        wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            required_features: wgpu::Features::default(),
            ..Default::default()
        }
    }

    /// Creates the [`wgpu::SurfaceConfiguration`] to configure a [`wgpu::Surface`] with.
    ///
    /// Note that the input size may be zero and it is up to the implementor to ensure a non-zero size on the surface configuration.
    fn surface_configuration(
        surface: &wgpu::Surface,
        adapter: &wgpu::Adapter,
        size: winit::dpi::PhysicalSize<u32>,
    ) -> wgpu::SurfaceConfiguration {
        surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .unwrap()
    }

    /// Creates the [`wgpu::RequestAdapterOptions`] to request a [`wgpu::Adapter`] with.
    fn request_adapter_options<'a, 'b>(
        surface: &'a wgpu::Surface<'b>,
    ) -> wgpu::RequestAdapterOptions<'a, 'b> {
        wgpu::RequestAdapterOptions {
            compatible_surface: Some(surface),
            ..Default::default()
        }
    }

    /// Creates a new instance of this application.
    fn new(user_event_sender: UserEventSender<Self::UserEvent>) -> Self;

    /// Handles application resumption.
    ///
    /// - [`Context::window`]\: Available.
    /// - [`Context::wgpu`]\: Available.
    ///
    /// See [`winit::application::ApplicationHandler::resumed`] for more details.
    fn resumed(&mut self, ctxt: &Context) {
        let _ = ctxt;
    }

    /// Handles application memory warnings.
    ///
    /// - [`Context::window`]\: Available.
    /// - [`Context::wgpu`]\: Available.
    ///
    /// See [`winit::application::ApplicationHandler::memory_warning`] for more details.
    fn memory_warning(&mut self, ctxt: &Context) {
        let _ = ctxt;
    }

    /// Handles when the application receives new events ready to be processed.
    ///
    /// - [`Context::window`]\: Not available.
    /// - [`Context::wgpu`]\: Not available.
    ///
    /// See [`winit::application::ApplicationHandler::new_events`] for more details.
    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        start_cause: winit::event::StartCause,
    ) {
        let _ = (event_loop, start_cause);
    }

    /// Handles when the application is about to block and wait for new events.
    ///
    /// - [`Context::window`]\: Available.
    /// - [`Context::wgpu`]\: May or may not be available.
    ///
    /// See [`winit::application::ApplicationHandler::about_to_wait`] for more details.
    fn about_to_wait(&mut self, ctxt: &Context) {
        let _ = ctxt;
    }

    /// Handles application suspension.
    ///
    /// - [`Context::window`]\: Available.
    /// - [`Context::wgpu`]\: Not available.
    ///
    /// See [`winit::application::ApplicationHandler::suspended`] for more details.
    fn suspended(&mut self, ctxt: &Context) {
        let _ = ctxt;
    }

    /// Handles application exiting.
    ///
    /// - [`Context::window`]\: Available.
    /// - [`Context::wgpu`]\: Available.
    ///
    /// See [`winit::application::ApplicationHandler::exiting`] for more details.
    fn exiting(&mut self, ctxt: &Context) {
        let _ = ctxt;
    }

    /// Handles a window event.
    ///
    /// wginit will handle [`winit::event::WindowEvent::Resized`] to update the size of the wgpu surface. You must handle all other events yourself.
    ///
    /// - [`Context::window`]\: Available.
    /// - [`Context::wgpu`]\: May or may not be available.
    ///
    /// See [`winit::application::ApplicationHandler::window_event`] for more details.
    fn window_event(&mut self, ctxt: &Context, event: winit::event::WindowEvent) {
        let _ = (ctxt, event);
    }

    /// Handles a user event.
    ///
    /// User events can be sent using [`UserEventSender`].
    ///
    /// - [`Context::window`]\: May or may not be available.
    /// - [`Context::wgpu`]\: May or may not be available.
    fn user_event(&mut self, ctxt: &Context, event: Self::UserEvent) {
        let _ = (ctxt, event);
    }

    /// Handles a device event.
    ///
    /// - [`Context::window`]\: May or may not be available.
    /// - [`Context::wgpu`]\: May or may not be available.
    ///
    /// See [`winit::application::ApplicationHandler::device_event`] for more details.
    fn device_event(
        &mut self,
        ctxt: &Context,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        let _ = (ctxt, device_id, event);
    }

    /// Handles a redraw request.
    ///
    /// It will run whenever [`winit::event::WindowEvent::RedrawRequested`] is emitted *and* wgpu is initialized.
    fn redraw(&mut self, window: &winit::window::Window, wgpu: &Wgpu) {
        let _ = (window, wgpu);
    }
}

/// Runs the application.
///
/// This will set up the event loop and run the application.
pub fn run<A>() -> Result<(), winit::error::EventLoopError>
where
    A: ApplicationHandler,
{
    let event_loop = winit::event_loop::EventLoop::with_user_event().build()?;
    let mut app = WinitApplicationHandler::new(
        A::new(UserEventSender(event_loop.create_proxy())),
        &event_loop,
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}
