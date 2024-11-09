//! wginit is a simple framework for initializing wgpu + winit.

/// The graphics device state.
///
/// It contains all wgpu and winit state.
pub struct Graphics {
    /// The current [`wgpu::Device`].
    pub window: std::sync::Arc<winit::window::Window>,
    /// The current [`wgpu::Queue`].
    pub device: wgpu::Device,
    /// The current [`wgpu::Adapter`].
    pub queue: wgpu::Queue,
    /// The current [`wgpu::Surface`].
    pub adapter: wgpu::Adapter,
    /// The current [`winit::window::Window`].
    pub surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
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

impl Graphics {
    pub(crate) async fn new<A>(window: winit::window::Window) -> Self
    where
        A: Application,
    {
        let window = std::sync::Arc::new(window);

        let instance = new_wgpu_instance().await;
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(&A::device_descriptor(&adapter), None)
            .await
            .expect("failed to create device");

        let mut size = window.inner_size();
        size.width = size.width.max(1);
        size.height = size.height.max(1);

        let mut surface_config = surface
            .get_default_config(&adapter, size.width, size.height)
            .unwrap();
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &surface_config);

        window.request_redraw();

        Self {
            window,
            device,
            queue,
            adapter,
            surface,
            surface_config,
        }
    }

    pub(crate) fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.surface_config.width = size.width.max(1);
        self.surface_config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.window.request_redraw();
    }
}

enum UserEvent<C> {
    GraphicsReady(Graphics),
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
    gfx: Option<Graphics>,
    event_loop_proxy: winit::event_loop::EventLoopProxy<UserEvent<A::UserEvent>>,
    app: Option<A>,
}

impl<A> ApplicationHandler<A>
where
    A: Application,
{
    fn new(event_loop: &winit::event_loop::EventLoop<UserEvent<A::UserEvent>>) -> Self {
        Self {
            gfx: None,
            event_loop_proxy: event_loop.create_proxy(),
            app: None,
        }
    }
}

impl<A> winit::application::ApplicationHandler<UserEvent<A::UserEvent>> for ApplicationHandler<A>
where
    A: Application,
{
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = event_loop
            .create_window(A::window_attrs())
            .expect("failed to create window");

        let event_loop_proxy = self.event_loop_proxy.clone();
        let fut = async move {
            assert!(event_loop_proxy
                .send_event(UserEvent::GraphicsReady(Graphics::new::<A>(window).await))
                .is_ok());
        };

        #[cfg(not(target_arch = "wasm32"))]
        pollster::block_on(fut);

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(fut);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Some(gfx) = &mut self.gfx else {
            return;
        };

        let Some(app) = self.app.as_mut() else {
            return;
        };

        app.window_event(&event);

        match event {
            winit::event::WindowEvent::Resized(size) => {
                gfx.resize(size);
            }
            winit::event::WindowEvent::RedrawRequested => {
                app.redraw(gfx);
            }
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
        };
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserEvent<A::UserEvent>,
    ) {
        match event {
            UserEvent::GraphicsReady(mut gfx) => {
                self.app = Some(A::new(
                    &mut gfx,
                    UserEventSender(self.event_loop_proxy.clone()),
                ));
                self.gfx = Some(gfx);
            }
            UserEvent::Custom(e) => {
                let Some(app) = self.app.as_mut() else {
                    return;
                };
                app.user_event(&e);
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
pub trait Application
where
    Self::UserEvent: 'static,
{
    /// The type of user event for this application.
    ///
    /// If no user events are desired, you can use `()` for the type.
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

    /// Creates the device descriptor to create a [`wgpu::Device`] with.
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

    /// Creates a new instance of this application.
    fn new(gfx: &mut Graphics, user_event_sender: UserEventSender<Self::UserEvent>) -> Self;

    /// Processes a redraw request.
    fn redraw(&mut self, gfx: &Graphics);

    /// Handles a window event.
    ///
    /// Note that on [`winit::event::WindowEvent::RedrawRequested`], both this and [`Application::redraw`] will be called, but [`Graphics`] will only be available from [`Application::redraw`].
    fn window_event(&mut self, event: &winit::event::WindowEvent) {
        _ = event;
    }

    /// Handles a user event.
    ///
    /// User events can be sent using [`UserEventSender`].
    fn user_event(&mut self, event: &Self::UserEvent) {
        _ = event;
    }
}

/// Runs the application.
///
/// This should be the only function called in your `main`. It will:
/// - Set up logging (and panic handling for WASM).
/// - Create the event loop.
/// - Starts the event loop and hands over control.
pub fn run<A>()
where
    A: Application,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::init();
    }

    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        wasm_logger::init(wasm_logger::Config::default());
    }

    let event_loop = winit::event_loop::EventLoop::with_user_event()
        .build()
        .unwrap();
    let mut app = ApplicationHandler::<A>::new(&event_loop);
    event_loop.run_app(&mut app).unwrap();
}
