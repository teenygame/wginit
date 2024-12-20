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
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
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

    fn redraw(&mut self, window: &winit::window::Window, wgpu: &wginit::Wgpu) {
        let gfx_state = self.gfx_state.as_ref().unwrap();
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
}

fn main() {
    wginit::run::<Application>().unwrap();
}
