pub mod plane_fit {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/plane_fit.rs"));
}
pub mod scan_image {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/scan_image.rs"));

    use crate::buffers::ColorMapTexture;
    use crate::buffers::TransformBuffer;
    pub use bind_groups::BindGroup0 as GlobalBindGroup;
    pub use bind_groups::BindGroup1 as LocalBindGroup;
    use wgpu::BlendState;
    use wgpu::ColorTargetState;
    use wgpu::ColorWrites;
    use wgpu::Device;
    use wgpu::FilterMode;
    use wgpu::MultisampleState;
    use wgpu::PrimitiveState;
    use wgpu::PrimitiveTopology;
    use wgpu::RenderPipeline;
    use wgpu::RenderPipelineDescriptor;
    use wgpu::SamplerDescriptor;
    use wgpu::TextureFormat;
    use wgpu::TextureViewDescriptor;

    pub fn create_main_pipeline(device: &Device, target_format: TextureFormat) -> RenderPipeline {
        let shader_module = create_shader_module(device);
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&create_pipeline_layout(device)),
            vertex: vertex_state(&shader_module, &vs_main_entry()),
            fragment: Some(fragment_state(
                &shader_module,
                &fs_main_entry([Some(ColorTargetState {
                    format: target_format,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })]),
            )),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 4,
                mask: !0,
                alpha_to_coverage_enabled: true,
            },
            multiview: None,
            cache: None,
        })
    }
    impl GlobalBindGroup {
        pub fn new(
            device: &Device,
            screen_transform_buf: &TransformBuffer,
            color_map_texture: &ColorMapTexture,
        ) -> Self {
            let sampler = device.create_sampler(&SamplerDescriptor {
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..Default::default()
            });
            bind_groups::BindGroup0::from_bindings(
                device,
                bind_groups::BindGroupLayout0 {
                    world2screen: screen_transform_buf.0.as_entire_buffer_binding(),
                    tex_sampler: &sampler,
                    color_map: &color_map_texture
                        .0
                        .create_view(&TextureViewDescriptor::default()),
                },
            )
        }
    }
}
pub mod plane_fit_32 {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/plane_fit_32.rs"));
}
