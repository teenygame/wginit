//! wginit is a simple framework for initializing wgpu + winit.

/// The graphics device state.
///
/// It contains all wgpu and winit state.
pub struct Graphics<'a> {
    /// The current [`wgpu::Device`].
    pub window: &'a winit::window::Window,
    /// The current [`wgpu::Queue`].
    pub device: &'a wgpu::Device,
    /// The current [`wgpu::Adapter`].
    pub queue: &'a wgpu::Queue,
    /// The current [`wgpu::Surface`].
    pub adapter: &'a wgpu::Adapter,
    /// The current [`winit::window::Window`].
    pub surface: &'a wgpu::Surface<'static>,
}

impl<'a> Graphics<'a> {
    fn new(window: &'a winit::window::Window, wgpu: &'a WgpuState) -> Self {
        Self {
            window: &window,
            device: &wgpu.device,
            queue: &wgpu.queue,
            adapter: &wgpu.adapter,
            surface: &wgpu.surface,
        }
    }
}

struct WgpuState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
}

impl WgpuState {
    async fn new<A>(window: std::sync::Arc<winit::window::Window>) -> Self
    where
        A: Application,
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
    WgpuReady(WgpuState),
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

struct ApplicationHandler<A>
where
    A: Application,
{
    app: A,
    window: Option<std::sync::Arc<winit::window::Window>>,
    wgpu: Option<WgpuState>,
    event_loop_proxy: winit::event_loop::EventLoopProxy<UserEvent<A::UserEvent>>,
}

impl<A> ApplicationHandler<A>
where
    A: Application,
{
    fn new(app: A, event_loop: &winit::event_loop::EventLoop<UserEvent<A::UserEvent>>) -> Self {
        Self {
            app,
            window: None,
            wgpu: None,
            event_loop_proxy: event_loop.create_proxy(),
        }
    }
}

impl<A> winit::application::ApplicationHandler<UserEvent<A::UserEvent>> for ApplicationHandler<A>
where
    A: Application,
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
                .send_event(UserEvent::WgpuReady(WgpuState::new::<A>(window).await))
                .is_ok());
        };

        #[cfg(not(target_arch = "wasm32"))]
        pollster::block_on(fut);

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(fut);
    }

    fn suspended(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.app.suspended();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let (Some(window), Some(wgpu)) = (&self.window, &self.wgpu) else {
            return;
        };

        match event {
            winit::event::WindowEvent::Resized(size) => {
                wgpu.surface.configure(
                    &wgpu.device,
                    &A::surface_configuration(&wgpu.surface, &wgpu.adapter, size),
                );
                window.request_redraw();
            }
            winit::event::WindowEvent::RedrawRequested => {
                self.app.redraw(&Graphics::new(window, wgpu));
            }
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
        };

        self.app.window_event(event);
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserEvent<A::UserEvent>,
    ) {
        match event {
            UserEvent::WgpuReady(wgpu) => {
                // We can just unwrap here because if we're getting the wgpu state we can safely assume the window is already initialized, otherwise we have bigger problems.
                let window = self.window.as_mut().unwrap();
                window.request_redraw();
                self.app.resumed(&Graphics::new(window, &wgpu));
                self.wgpu = Some(wgpu);
            }
            UserEvent::Custom(e) => {
                self.app.user_event(e);
            }
        }
    }
}

/// The application.
///
/// You should implement all the methods in this trait.
///
/// The following is handled for you:
/// - Window creation
/// - Surface resizing
/// - Window closing
///
/// You should handle the following yourself:
/// - Inputs (via [`Application::window_event`])
/// - Drawing (via [`Application::redraw`])
/// - Resumption (via [`Application::resumed`])
/// - Suspension (via [`Application::suspended`])
///
/// You may override various aspects of winit/wgpu initialization, e.g.:
/// - [`wgpu::DeviceDescriptor`] (via [`Application::device_descriptor`])
/// - [`wgpu::SurfaceConfiguration`] (via [`Application::surface_configuration`])
/// - [`wgpu::RequestAdapterOptions`] (via [`Application::request_adapter_options`])
/// - [`winit::window::WindowAttributes`] (via [`Application::window_attrs`])
///
/// Additionally, events can be delivered to the event loop via the [`UserEventSender`] passed to [`Application::new`]. If used, they can be handled via [`Application::user_event`].
pub trait Application
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
    /// This function will be called when the graphics context is initialized.
    fn resumed(&mut self, gfx: &Graphics) {
        _ = gfx;
    }

    /// Handles application suspension.
    fn suspended(&mut self) {}

    /// Processes a redraw request.
    fn redraw(&mut self, gfx: &Graphics);

    /// Handles a window event.
    ///
    /// Note that on [`winit::event::WindowEvent::RedrawRequested`], both this and [`Application::redraw`] will be called, but [`Graphics`] will only be available from [`Application::redraw`].
    fn window_event(&mut self, event: winit::event::WindowEvent) {
        _ = event;
    }

    /// Handles a user event.
    ///
    /// User events can be sent using [`UserEventSender`].
    fn user_event(&mut self, event: Self::UserEvent) {
        _ = event;
    }
}

/// Runs the application.
///
/// This will set up the event loop and run the application.
pub fn run<A>() -> Result<(), winit::error::EventLoopError>
where
    A: Application,
{
    let event_loop = winit::event_loop::EventLoop::with_user_event().build()?;
    let mut app = ApplicationHandler::new(
        A::new(UserEventSender(event_loop.create_proxy())),
        &event_loop,
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}
