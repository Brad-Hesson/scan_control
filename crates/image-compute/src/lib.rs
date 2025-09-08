#![allow(dead_code)]
use std::sync::{Arc, OnceLock};

use egui::Color32;
use glam::Affine2;
use itertools::{Itertools, izip};
use tracing::info;
use wgpu::{
    BlendState, BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, ComputePass,
    ComputePipeline, Device, Extent3d, FilterMode, MultisampleState, PrimitiveState,
    PrimitiveTopology, QuerySet, QueryType, Queue, RenderPass, RenderPipeline,
    RenderPipelineDescriptor, SamplerDescriptor, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor, util::align_to, wgt::QuerySetDescriptor,
};

use crate::{
    buffers::{ColorMapTexture, StorageBuffer, TransformBuffer},
    shaders::{plane_fit, scan_image},
};

mod buffers;
mod shaders;

pub struct ImageComputeBuffers {
    size: [u32; 2],
    lines: u32,
    world_transform_buffer: TransformBuffer,
    image_size_buffer: StorageBuffer<u32>,
    image_data_buffer: StorageBuffer<f32>,
    planarize_buffer: StorageBuffer<f64>,
    normalize_buffer: StorageBuffer<plane_fit::NormalizeData>,
    image_src_bg: plane_fit::bind_groups::BindGroup0,
    normalize_bg: plane_fit::bind_groups::BindGroup1,
    scan_image_bg: scan_image::bind_groups::BindGroup1,
}
impl ImageComputeBuffers {
    pub fn new(
        device: &Device,
        label: Option<&str>,
        size: [u32; 2],
        lines: u32,
        init_fn: impl FnOnce(&mut [f32]),
    ) -> Self {
        let size_buffer_label = label.map(|name| format!("{name}_size_buffer"));
        let image_size_buffer = buffers::StorageBuffer::new(
            &device,
            size_buffer_label.as_deref(),
            BufferUsages::UNIFORM,
            2,
            |buf| {
                buf[0] = size[0];
                buf[1] = lines;
            },
        );
        let data_buffer_label = label.map(|name| format!("{name}_data_buffer"));
        let image_data_buffer = buffers::StorageBuffer::new(
            &device,
            data_buffer_label.as_deref(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            size[0] as usize * size[1] as usize,
            init_fn,
        );
        let world_transform_buffer = TransformBuffer::new(device);
        let image_texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[TextureFormat::R32Float],
        });
        let normalize_control = StorageBuffer::new(
            device,
            None,
            BufferUsages::UNIFORM | BufferUsages::COPY_SRC,
            1,
            |data| {
                data[0] = scan_image::NormalizeControl {
                    max_min: 1,
                    std_dev_mul: 5.,
                    _pad: 0,
                }
            },
        );
        let normalize_buffer = StorageBuffer::<plane_fit::NormalizeData>::new(
            &device,
            Some("normalize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::UNIFORM,
            1,
            |_| {},
        );
        let planarize_buffer = StorageBuffer::<f64>::new(
            &device,
            Some("planarize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            size[0] as usize * size[1] as usize,
            |_| {},
        );
        let image_src_bg = plane_fit::bind_groups::BindGroup0::from_bindings(
            &device,
            plane_fit::bind_groups::BindGroupLayout0 {
                image_size: image_size_buffer.as_entire_buffer_binding(),
                image_in: image_data_buffer.as_entire_buffer_binding(),
            },
        );
        let scan_image_bg = scan_image::bind_groups::BindGroup1::from_bindings(
            device,
            scan_image::bind_groups::BindGroupLayout1 {
                quad2world: world_transform_buffer.as_entire_buffer_binding(),
                height_map: &image_texture.create_view(&TextureViewDescriptor::default()),
                normalize_data: normalize_buffer.as_entire_buffer_binding(),
                normalize_control: normalize_control.as_entire_buffer_binding(),
            },
        );
        let normalize_bg = plane_fit::bind_groups::BindGroup1::from_bindings(
            &device,
            plane_fit::bind_groups::BindGroupLayout1 {
                texture_out: &image_texture.create_view(&TextureViewDescriptor::default()),
                planarize_out: planarize_buffer.as_entire_buffer_binding(),
                normalize_out: normalize_buffer.as_entire_buffer_binding(),
            },
        );
        Self {
            image_size_buffer,
            image_data_buffer,
            image_src_bg,
            size,
            lines,
            scan_image_bg,
            world_transform_buffer,
            planarize_buffer,
            normalize_buffer,
            normalize_bg,
        }
    }
    pub fn write_world_transform(&self, queue: &Queue, transform: Affine2) {
        self.world_transform_buffer.write(queue, transform);
    }
    pub fn current_size(&self) -> [u32; 2] {
        [self.size[0], self.lines]
    }
}

#[allow(non_snake_case)]
struct ImageComputeScratchBuffers {
    xz: StorageBuffer<f64>,
    yz: StorageBuffer<f64>,
    std_dev: StorageBuffer<f64>,
    bg: plane_fit::bind_groups::BindGroup2,
    size: [u32; 2],
}
impl ImageComputeScratchBuffers {
    fn new(device: &Device, size: [u32; 2]) -> Self {
        let mk_buffer = |s: &'static str| {
            StorageBuffer::new(
                device,
                Some(s),
                BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                size[0] as usize * size[1] as usize,
                |_| {},
            )
        };
        let xz = mk_buffer("xz");
        let yz = mk_buffer("yz");
        let std_dev = mk_buffer("std_dev");
        Self {
            size,
            bg: plane_fit::bind_groups::BindGroup2::from_bindings(
                &device,
                plane_fit::bind_groups::BindGroupLayout2 {
                    xz: xz.as_entire_buffer_binding(),
                    yz: yz.as_entire_buffer_binding(),
                    std_dev: std_dev.as_entire_buffer_binding(),
                },
            ),
            xz,
            yz,
            std_dev,
        }
    }
}

pub struct ImageComputePipeline {
    copy_image: ComputePipeline,
    copy_image_transpose: ComputePipeline,
    generate_sums_plane: ComputePipeline,
    generate_sums_lines: ComputePipeline,
    reduce_image: ComputePipeline,
    reduce_image_lines: ComputePipeline,
    reduce_sums_plane: ComputePipeline,
    reduce_sums_lines: ComputePipeline,
    reduce_normalizations: ComputePipeline,
    generate_normalization__mean_subtract: ComputePipeline,
    write__mean_subtract: ComputePipeline,
    qs: QuerySet,
    qs_buf: StorageBuffer<u64>,
    scratch_buffers: ImageComputeScratchBuffers,
}
impl ImageComputePipeline {
    pub fn new(device: &Device) -> Self {
        let n_timings = 5;
        Self {
            copy_image: plane_fit::compute::create_copy_image_pipeline(device),
            copy_image_transpose: plane_fit::compute::create_copy_image_transpose_pipeline(device),
            generate_sums_plane: plane_fit::compute::create_generate_sums_plane_pipeline(device),
            generate_sums_lines: plane_fit::compute::create_generate_sums_lines_pipeline(device),
            reduce_image: plane_fit::compute::create_reduce_image_pipeline(device),
            reduce_image_lines: plane_fit::compute::create_reduce_image_lines_pipeline(device),
            reduce_sums_plane: plane_fit::compute::create_reduce_sums_plane_pipeline(device),
            reduce_sums_lines: plane_fit::compute::create_reduce_sums_lines_pipeline(device),
            reduce_normalizations: plane_fit::compute::create_reduce_normalizations_pipeline(
                device,
            ),
            generate_normalization__mean_subtract:
                plane_fit::compute::create_generate_normalization__mean_subtract_pipeline(device),
            write__mean_subtract: plane_fit::compute::create_write__mean_subtract_pipeline(device),
            qs: device.create_query_set(&QuerySetDescriptor {
                label: Some("plane_fitter_qs"),
                ty: QueryType::Timestamp,
                count: n_timings * 2,
            }),
            qs_buf: StorageBuffer::new(
                device,
                Some("plane_fitter_qs_buf"),
                BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
                n_timings as usize * 2,
                |_| {},
            ),
            scratch_buffers: ImageComputeScratchBuffers::new(device, [1024, 1024]),
        }
    }
    pub fn dispatch_mean_subtract(
        &mut self,
        device: &Device,
        pass: &mut ComputePass,
        image: &ImageComputeBuffers,
    ) -> usize {
        let mut qs_n = 0;
        let mut wts = |pass: &mut ComputePass| {
            pass.write_timestamp(&self.qs, qs_n as u32);
            qs_n += 1;
        };
        if izip!(self.scratch_buffers.size, image.size).any(|(a, b)| a < b) {
            info!("Reallocating scratch buffers to {:?}", image.size);
            self.scratch_buffers = ImageComputeScratchBuffers::new(device, image.size);
        }

        self.scratch_buffers.bg.set(pass);
        image.image_src_bg.set(pass);
        image.normalize_bg.set(pass);
        let size = image.current_size();

        pass.set_pipeline(&self.copy_image);
        wts(pass);
        dispatch_linear(pass, size);
        wts(pass);

        pass.set_pipeline(&self.reduce_image);
        wts(pass);
        dispatch_reduction(pass, size);
        wts(pass);

        pass.set_pipeline(&self.generate_normalization__mean_subtract);
        wts(pass);
        dispatch_linear(pass, size);
        wts(pass);

        pass.set_pipeline(&self.reduce_normalizations);
        wts(pass);
        dispatch_reduction(pass, size);
        wts(pass);

        pass.set_pipeline(&self.write__mean_subtract);
        wts(pass);
        dispatch_linear(pass, size);
        wts(pass);

        qs_n / 2
    }
    // pub fn run_subtract_plane(
    //     &self,
    //     pass: &mut ComputePass,
    //     scratch_buffers: &PlaneFitterBuffers,
    // ) -> usize {
    //     let mut qs_n = 0;
    //     let mut wts = |pass: &mut ComputePass| {
    //         pass.write_timestamp(&self.qs, qs_n as u32);
    //         qs_n += 1;
    //     };
    //     scratch_buffers.bg.set(pass);

    //     pass.set_pipeline(&self.copy_image);
    //     wts(pass);
    //     dispatch_linear(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.reduce_image);
    //     wts(pass);
    //     dispatch_reduction(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.generate_sums_plane);
    //     wts(pass);
    //     dispatch_linear(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.reduce_sums_plane);
    //     wts(pass);
    //     dispatch_reduction(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.subtract_plane);
    //     wts(pass);
    //     dispatch_linear(pass, scratch_buffers.size);
    //     wts(pass);

    //     qs_n / 2
    // }
    // pub fn run_subtract_lines(
    //     &self,
    //     pass: &mut ComputePass,
    //     scratch_buffers: &PlaneFitterBuffers,
    // ) -> usize {
    //     let mut qs_n = 0;
    //     let mut wts = |pass: &mut ComputePass| {
    //         pass.write_timestamp(&self.qs, qs_n as u32);
    //         qs_n += 1;
    //     };
    //     scratch_buffers.bg.set(pass);

    //     pass.set_pipeline(&self.copy_image_transpose);
    //     wts(pass);
    //     dispatch_linear(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.reduce_image_lines);
    //     wts(pass);
    //     dispatch_y_reduction(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.generate_sums_lines);
    //     wts(pass);
    //     dispatch_linear(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.reduce_sums_lines);
    //     wts(pass);
    //     dispatch_y_reduction(pass, scratch_buffers.size);
    //     wts(pass);

    //     pass.set_pipeline(&self.subtract_lines);
    //     wts(pass);
    //     dispatch_linear(pass, scratch_buffers.size);
    //     wts(pass);

    //     qs_n / 2
    // }
    pub fn queue_timings_download(
        &self,
        device: &Device,
        queue: &Queue,
        num: usize,
    ) -> Arc<OnceLock<Box<[u64]>>> {
        self.qs_buf
            .queue_download_with(device, queue, ..num * 2, |r| {
                r.iter()
                    .chunks(2)
                    .into_iter()
                    .map(|c| c.collect_tuple().unwrap())
                    .map(|(a, b)| b.saturating_sub(*a))
                    .collect_vec()
                    .into_boxed_slice()
            })
    }
    pub fn resolve_timings(&self, encoder: &mut CommandEncoder, num: usize) {
        encoder.resolve_query_set(&self.qs, 0..num as u32 * 2, self.qs_buf.buffer_ref(), 0);
    }
}

fn dispatch_linear(pass: &mut ComputePass, size: [u32; 2]) {
    pass.dispatch_workgroups(
        align_to(size[0] * size[1], plane_fit::WGS) / plane_fit::WGS,
        1,
        1,
    );
}

fn dispatch_reduction(pass: &mut ComputePass, size: [u32; 2]) {
    let mut remaining_data = size[0] * size[1];
    while remaining_data > 1 {
        let num_workgroups = align_to(remaining_data, plane_fit::WGS) / plane_fit::WGS;
        pass.dispatch_workgroups(num_workgroups, 1, 1);
        remaining_data = num_workgroups;
    }
}

fn dispatch_y_reduction(pass: &mut ComputePass, size: [u32; 2]) {
    let mut remaining_data = size[0];
    while remaining_data > 1 {
        let num_workgroups =
            align_to(remaining_data, plane_fit::WGS_SQUARE) / plane_fit::WGS_SQUARE;
        pass.dispatch_workgroups(
            align_to(size[1], plane_fit::WGS_SQUARE) / plane_fit::WGS_SQUARE,
            num_workgroups,
            1,
        );
        remaining_data = num_workgroups;
    }
}

pub struct ScanImageBuffers<const COLOR_MAP_SIZE: usize> {
    screen_transform_buffer: TransformBuffer,
    color_map_texture: ColorMapTexture<COLOR_MAP_SIZE>,
    bg: shaders::scan_image::bind_groups::BindGroup0,
}

impl<const COLOR_MAP_SIZE: usize> ScanImageBuffers<COLOR_MAP_SIZE> {
    pub fn new(device: &Device) -> Self {
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let screen_transform_buffer = TransformBuffer::new(device);
        let color_map_texture = ColorMapTexture::new(device);
        let bg = shaders::scan_image::bind_groups::BindGroup0::from_bindings(
            device,
            shaders::scan_image::bind_groups::BindGroupLayout0 {
                world2screen: screen_transform_buffer.as_entire_buffer_binding(),
                tex_sampler: &sampler,
                color_map: &color_map_texture.create_view(),
            },
        );
        Self {
            screen_transform_buffer,
            color_map_texture,
            bg,
        }
    }
    pub fn write_screen_transform(&self, queue: &Queue, transform: Affine2) {
        self.screen_transform_buffer.write(queue, transform);
    }
    pub fn write_color_map(&self, queue: &Queue, color_map: &[Color32; COLOR_MAP_SIZE]) {
        self.color_map_texture.write(queue, color_map);
    }
}

pub struct ScanImagePipeline {
    pipeline: RenderPipeline,
}
impl ScanImagePipeline {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let shader_module = shaders::scan_image::create_shader_module(device);
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&shaders::scan_image::create_pipeline_layout(device)),
            vertex: scan_image::vertex_state(&shader_module, &scan_image::vs_main_entry()),
            fragment: Some(scan_image::fragment_state(
                &shader_module,
                &scan_image::fs_main_entry([Some(ColorTargetState {
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
        });
        Self { pipeline }
    }
    pub fn draw<const COLOR_MAP_SIZE: usize>(
        &self,
        pass: &mut RenderPass,
        image_buffers: &ImageComputeBuffers,
        scan_image_buffers: &ScanImageBuffers<COLOR_MAP_SIZE>,
    ) {
        pass.set_pipeline(&self.pipeline);
        scan_image_buffers.bg.set(pass);
        image_buffers.scan_image_bg.set(pass);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use eyre::{Context, Result};
    use itertools::izip;
    use tracing::info;
    use tracing_subscriber::EnvFilter;
    use wgpu::{
        Adapter, CommandEncoderDescriptor, ComputePassDescriptor, Device, DeviceDescriptor,
        Extent3d, FeaturesWGPU, FeaturesWebGPU, Instance, PollType, PowerPreference, Queue,
        RequestAdapterOptions, TextureDescriptor, TextureFormat, TextureUsages,
        TextureViewDescriptor,
    };

    use crate::shaders::plane_fit::NormalizeData;

    use super::*;

    #[test]
    fn output() -> Result<()> {
        let (_instance, _adapter, device, queue) = init().context("Init failed")?;
        const WIDTH: usize = 512;
        const HEIGHT: usize = 1024;
        const SIZE: [u32; 2] = [WIDTH as _, HEIGHT as _];
        let mut plane_fitter = ImageComputePipeline::new(&device);
        let x_slope = 1.0;
        let y_slope = 10.0;
        let offset = 0.0;
        let mut mean = 0.;
        let init_data = |data: &mut [f32]| {
            // data.fill(1.);
            // for i in 0..data.len() {
            //     data[i] = (i / 32) as f32;
            // }
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let dat = &mut data[y * WIDTH + x];
                    let (x, y) = (x as f32 / WIDTH as f32, y as f32 / HEIGHT as f32);
                    *dat = (x_slope / WIDTH as f32) * x + (y_slope / HEIGHT as f32) * y;
                    let val = x_slope * x + y_slope * y + offset;
                    // *dat = y as f32;
                    // mean += val as f64;
                }
            }
        };
        let original = ImageComputeBuffers::new(
            &device,
            Some("original_image"),
            SIZE,
            HEIGHT as u32,
            init_data,
        );
        mean /= (WIDTH * HEIGHT) as f64;
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        unsafe { device.start_graphics_debugger_capture() };
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("test_name"),
        });
        let n_times;
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Test Compute Pass"),
                timestamp_writes: None,
            });
            n_times = plane_fitter.dispatch_mean_subtract(&device, &mut pass, &original);
        }
        plane_fitter.resolve_timings(&mut encoder, n_times);
        device.poll(PollType::WaitForSubmissionIndex(
            queue.submit([encoder.finish()]),
        ))?;
        unsafe { device.stop_graphics_debugger_capture() };
        let meta_download = original
            .planarize_buffer
            .queue_download(&device, &queue, ..16);
        let normal_download = original
            .normalize_buffer
            .queue_download(&device, &queue, ..1);
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        println!(
            "0: {}, 1: {}, 2: {}",
            meta_download.get().unwrap()[0],
            meta_download.get().unwrap()[1],
            meta_download.get().unwrap()[2],
        );
        println!("Actual:");
        println!("a: {}, x: {}, y: {}", mean, x_slope, y_slope);
        Ok(())
    }
    #[test]
    fn timing() -> Result<()> {
        let (_instance, _adapter, device, queue) = init().context("Init failed")?;
        const WIDTH: usize = 1024;
        const HEIGHT: usize = 1024;
        const SIZE: [u32; 2] = [WIDTH as _, HEIGHT as _];
        let mut plane_fitter = ImageComputePipeline::new(&device);
        let original =
            ImageComputeBuffers::new(&device, Some("original_image"), SIZE, HEIGHT as u32, |_| {});
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        let mut times = vec![0., 0., 0., 0., 0.];
        let mut latest = vec![0., 0., 0., 0., 0.];
        let mult = queue.get_timestamp_period();
        for i in 1.. {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("test_name"),
            });
            let n_times;
            {
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("Test Compute Pass"),
                    timestamp_writes: None,
                });
                n_times = plane_fitter.dispatch_mean_subtract(&device, &mut pass, &original);
            }
            plane_fitter.resolve_timings(&mut encoder, n_times);
            device.poll(PollType::WaitForSubmissionIndex(
                queue.submit([encoder.finish()]),
            ))?;
            let times_download = plane_fitter.queue_timings_download(&device, &queue, n_times);
            device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
            let new_times = times_download
                .get()
                .unwrap()
                .iter()
                .map(|v| *v as f64 / 1000. * mult as f64);
            let x = 1. / (i as f64);
            for (mean, late, new) in izip!(times.iter_mut(), latest.iter_mut(), new_times) {
                *late = new;
                *mean = *mean * (1. - x) + new * x;
            }
            if i % 100 == 0 {
                println!(
                    "            {latest:9.4?} -> {:9.4} micros",
                    latest.iter().sum::<f64>()
                );
                println!(
                    "{x:11.6} {times:9.4?} -> {:9.4} micros",
                    times.iter().sum::<f64>()
                );
                println!();
            }
        }
        Ok(())
    }
    fn lerp(s: f64, e: f64, t: f64) -> f64 {
        t * e + (1. - t) * s
    }
    fn init() -> Result<(Instance, Adapter, Device, Queue)> {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        let instance = wgpu::Instance::default();
        let adapter = smol::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .context("Adapter request failed")?;
        info!("Backend: {}", adapter.get_info().backend.to_str());
        info!("Adapter: {}", adapter.get_info().name);
        info!(
            "Driver: {} {}",
            adapter.get_info().driver,
            adapter.get_info().driver_info
        );
        let (dev, queue) = smol::block_on(adapter.request_device(&DeviceDescriptor {
            required_features: wgpu::Features {
                features_wgpu: FeaturesWGPU::TIMESTAMP_QUERY_INSIDE_PASSES
                    | FeaturesWGPU::SHADER_F64
                    | FeaturesWGPU::SHADER_INT64,
                features_webgpu: FeaturesWebGPU::FLOAT32_FILTERABLE
                    | FeaturesWebGPU::TIMESTAMP_QUERY,
            },
            ..Default::default()
        }))
        .context("Device request failed")?;
        Ok((instance, adapter, dev, queue))
    }
}
