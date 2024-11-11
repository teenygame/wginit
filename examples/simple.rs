const SHADER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(in_vertex_index) - 1);
    let y = f32(i32(in_vertex_index & 1u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
"#;

struct GraphicsState {
    render_pipeline: wgpu::RenderPipeline,
}

impl GraphicsState {
    fn new(wgpu: &wginit::Wgpu) -> Self {
        let shader = wgpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER)),
            });

        let swapchain_format = wgpu.surface.get_capabilities(&wgpu.adapter).formats[0];

        Self {
            render_pipeline: wgpu
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: Some(&wgpu.device.create_pipeline_layout(
                        &wgpu::PipelineLayoutDescriptor {
                            label: None,
                            bind_group_layouts: &[],
                            push_constant_ranges: &[],
                        },
                    )),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: "fs_main",
                        targets: &[Some(swapchain_format.into())],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                }),
        }
    }
}

struct Application {
    gfx_state: Option<GraphicsState>,
}

impl wginit::ApplicationHandler for Application {
    type UserEvent = std::convert::Infallible;

    fn window_attrs() -> winit::window::WindowAttributes {
        #[allow(unused_mut)]
        let mut window_attrs = winit::window::WindowAttributes::default();
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys as _;
            window_attrs = window_attrs.with_append(true);
        }
        window_attrs = window_attrs.with_inner_size(winit::dpi::PhysicalSize::new(1024, 1024));
        window_attrs
    }

    fn new(_user_event_sender: wginit::UserEventSender<Self::UserEvent>) -> Self {
        Self { gfx_state: None }
    }

    fn resumed(&mut self, ctxt: &wginit::Context) {
        self.gfx_state = Some(GraphicsState::new(ctxt.wgpu.unwrap()));
    }

    fn suspended(&mut self, _ctxt: &wginit::Context) {
        self.gfx_state = None;
    }

    fn window_event(&mut self, ctxt: &wginit::Context, event: winit::event::WindowEvent) {
        match event {
            winit::event::WindowEvent::RedrawRequested => {
                let Some(gfx_state) = &self.gfx_state else {
                    return;
                };
                let wgpu = ctxt.wgpu.unwrap();
                let window = ctxt.window.unwrap();

                let frame = wgpu.surface.get_current_texture().unwrap();
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder = wgpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    rpass.set_pipeline(&gfx_state.render_pipeline);
                    rpass.draw(0..3, 0..1);
                }

                wgpu.queue.submit(Some(encoder.finish()));

                window.pre_present_notify();
                frame.present();
                window.request_redraw();
            }
            winit::event::WindowEvent::CloseRequested => {
                ctxt.event_loop.exit();
            }
            _ => {}
        }
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::init();
    }

    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        wasm_logger::init(wasm_logger::Config::default());
    }

    wginit::run::<Application>().unwrap();
}
