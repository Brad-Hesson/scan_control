#![allow(dead_code)]
use std::sync::{Arc, OnceLock};

use itertools::Itertools;
use wgpu::{
    BufferUsages, CommandEncoder, ComputePass, ComputePipeline, Device, QuerySet, QueryType, Queue,
    Texture, TextureViewDescriptor, util::align_to, wgt::QuerySetDescriptor,
};

use crate::{
    buffers::{ImageTexture, StorageBuffer, TransformBuffer},
    shaders::{
        plane_fit::{self, NormalizeData, bind_groups::BindGroup1},
        scan_image::{self, NormalizeControl},
    },
};

pub mod buffers;
pub mod shaders;

pub struct OutData {
    bind_group: BindGroup1,
    planarize_out: StorageBuffer<f64>,
    pub normalize_out: StorageBuffer<NormalizeData>,
}

impl OutData {
    pub fn new(device: &Device, size: [u32; 2], texture: &Texture) -> Self {
        let planarize_out = StorageBuffer::<f64>::new(
            &device,
            Some("planarize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            size[0] as usize * size[1] as usize,
            |_| {},
        );
        let normalize_out = StorageBuffer::<NormalizeData>::new(
            &device,
            Some("normalize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::UNIFORM,
            1,
            |_| {},
        );
        let bind_group = BindGroup1::from_bindings(
            &device,
            plane_fit::bind_groups::BindGroupLayout1 {
                texture_out: &texture.create_view(&TextureViewDescriptor::default()),
                planarize_out: planarize_out.inner.as_entire_buffer_binding(),
                normalize_out: normalize_out.inner.as_entire_buffer_binding(),
            },
        );
        Self {
            bind_group,
            planarize_out,
            normalize_out,
        }
    }
    pub fn set(&self, pass: &mut ComputePass) {
        self.bind_group.set(pass);
    }
}

pub struct Image {
    pub size: [u32; 2],
    pub world_transform_buffer: TransformBuffer,
    size_buffer: StorageBuffer<u32>,
    data_buffer: StorageBuffer<f32>,
    plane_fit_bg: plane_fit::bind_groups::BindGroup0,
    pub scan_image_bg: scan_image::bind_groups::BindGroup1,
    out_data: OutData,
}
impl Image {
    pub fn new(
        device: &Device,
        label: Option<&str>,
        size: [u32; 2],
        init_fn: impl FnOnce(&mut [f32]),
    ) -> Self {
        let size_buffer_label = label.map(|name| format!("{name}_size_buffer"));
        let size_buffer = buffers::StorageBuffer::new(
            &device,
            size_buffer_label.as_deref(),
            BufferUsages::UNIFORM,
            2,
            |buf| buf.copy_from_slice(&size),
        );
        let data_buffer_label = label.map(|name| format!("{name}_data_buffer"));
        let data_buffer = buffers::StorageBuffer::new(
            &device,
            data_buffer_label.as_deref(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            size.iter().map(|v| *v as usize).product(),
            init_fn,
        );
        let plane_fit_bg = plane_fit::bind_groups::BindGroup0::from_bindings(
            &device,
            plane_fit::bind_groups::BindGroupLayout0 {
                image_size: size_buffer.inner.as_entire_buffer_binding(),
                image_in: data_buffer.inner.as_entire_buffer_binding(),
            },
        );
        let world_transform_buffer = TransformBuffer::new(device);
        let image_texture = ImageTexture::new(device, size);
        let normalize_control = StorageBuffer::new(
            device,
            None,
            BufferUsages::UNIFORM | BufferUsages::COPY_SRC,
            1,
            |data| {
                data[0] = NormalizeControl {
                    max_min: 1,
                    _pad: 0,
                    std_dev_mul: 5.,
                }
            },
        );
        let out_data = OutData::new(device, size, &image_texture.0);
        let scan_image_bg = scan_image::bind_groups::BindGroup1::from_bindings(
            device,
            scan_image::bind_groups::BindGroupLayout1 {
                quad2world: world_transform_buffer.0.as_entire_buffer_binding(),
                height_map: &image_texture
                    .0
                    .create_view(&TextureViewDescriptor::default()),
                normalize_data: out_data.normalize_out.inner.as_entire_buffer_binding(),
                normalize_control: normalize_control.inner.as_entire_buffer_binding(),
            },
        );
        Self {
            size_buffer,
            data_buffer,
            plane_fit_bg,
            size,
            scan_image_bg,
            out_data,
            world_transform_buffer,
        }
    }
    pub fn set(&self, pass: &mut ComputePass) {
        self.plane_fit_bg.set(pass);
        self.out_data.set(pass);
    }
}

#[allow(non_snake_case)]
pub struct PlaneFitterBuffers {
    xz: StorageBuffer<f64>,
    yz: StorageBuffer<f64>,
    std_dev: StorageBuffer<f64>,
    bg: plane_fit::bind_groups::BindGroup2,
    size: [u32; 2],
}
impl PlaneFitterBuffers {
    pub fn new(device: &Device, size: [u32; 2]) -> Self {
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
                    xz: xz.inner.as_entire_buffer_binding(),
                    yz: yz.inner.as_entire_buffer_binding(),
                    std_dev: std_dev.inner.as_entire_buffer_binding(),
                },
            ),
            xz,
            yz,
            std_dev,
        }
    }
}

pub struct PlaneFitter {
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
}
impl PlaneFitter {
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
        }
    }
    pub fn run_mean_subtract(
        &self,
        pass: &mut ComputePass,
        scratch_buffers: &PlaneFitterBuffers,
    ) -> usize {
        let mut qs_n = 0;
        let mut wts = |pass: &mut ComputePass| {
            pass.write_timestamp(&self.qs, qs_n as u32);
            qs_n += 1;
        };
        scratch_buffers.bg.set(pass);

        pass.set_pipeline(&self.copy_image);
        wts(pass);
        dispatch_linear(pass, scratch_buffers.size);
        wts(pass);

        pass.set_pipeline(&self.reduce_image);
        wts(pass);
        dispatch_reduction(pass, scratch_buffers.size);
        wts(pass);

        pass.set_pipeline(&self.generate_normalization__mean_subtract);
        wts(pass);
        dispatch_linear(pass, scratch_buffers.size);
        wts(pass);

        pass.set_pipeline(&self.reduce_normalizations);
        wts(pass);
        dispatch_reduction(pass, scratch_buffers.size);
        wts(pass);

        pass.set_pipeline(&self.write__mean_subtract);
        wts(pass);
        dispatch_linear(pass, scratch_buffers.size);
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
        encoder.resolve_query_set(&self.qs, 0..num as u32 * 2, &self.qs_buf.inner, 0);
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

fn check_sizes<const N: usize>(images: [&Image; N]) -> Result<[u32; 2], TransformError> {
    let sizes = images.iter().map(|i| i.size).collect::<Vec<_>>();
    for size in &sizes[1..] {
        if *size != sizes[0] {
            return Err(TransformError::SizeMismatch(sizes));
        }
    }
    Ok(sizes[0])
}

#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    #[error("Size mismatch between image arguments: {:?}", 0)]
    SizeMismatch(Vec<[u32; 2]>),
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
        let plane_fitter = PlaneFitter::new(&device);
        let plane_fitter_buffers = PlaneFitterBuffers::new(&device, SIZE);
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
        let original = Image::new(&device, Some("original_image"), SIZE, init_data);
        mean /= (WIDTH * HEIGHT) as f64;
        let texture_out = device.create_texture(&TextureDescriptor {
            label: Some("texture_out"),
            size: Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[TextureFormat::R32Float],
        });
        let planarize_out = StorageBuffer::<f64>::new(
            &device,
            Some("planarize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            WIDTH * HEIGHT,
            |_| {},
        );
        let normalize_out = StorageBuffer::<NormalizeData>::new(
            &device,
            Some("normalize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            1,
            |_| {},
        );
        let out_bg = shaders::plane_fit::bind_groups::BindGroup1::from_bindings(
            &device,
            plane_fit::bind_groups::BindGroupLayout1 {
                texture_out: &texture_out.create_view(&TextureViewDescriptor::default()),
                planarize_out: planarize_out.inner.as_entire_buffer_binding(),
                normalize_out: normalize_out.inner.as_entire_buffer_binding(),
            },
        );
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
            original.set(&mut pass);
            out_bg.set(&mut pass);
            n_times = plane_fitter.run_mean_subtract(&mut pass, &plane_fitter_buffers);
        }
        plane_fitter.resolve_timings(&mut encoder, n_times);
        device.poll(PollType::WaitForSubmissionIndex(
            queue.submit([encoder.finish()]),
        ))?;
        unsafe { device.stop_graphics_debugger_capture() };
        let meta_download = planarize_out.queue_download(&device, &queue, ..16);
        let normal_download = normalize_out.queue_download(&device, &queue, ..1);
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
        let plane_fitter = PlaneFitter::new(&device);
        let plane_fitter_buffers = PlaneFitterBuffers::new(&device, SIZE);
        let original = Image::new(&device, Some("original_image"), SIZE, |_| {});
        let texture_out = device.create_texture(&TextureDescriptor {
            label: Some("texture_out"),
            size: Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[TextureFormat::R32Float],
        });
        let planarize_out = StorageBuffer::<f64>::new(
            &device,
            Some("planarize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            WIDTH * HEIGHT,
            |_| {},
        );
        let normalize_out = StorageBuffer::<NormalizeData>::new(
            &device,
            Some("normalize_out"),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            1,
            |_| {},
        );
        let out_bg = shaders::plane_fit::bind_groups::BindGroup1::from_bindings(
            &device,
            plane_fit::bind_groups::BindGroupLayout1 {
                texture_out: &texture_out.create_view(&TextureViewDescriptor::default()),
                planarize_out: planarize_out.inner.as_entire_buffer_binding(),
                normalize_out: normalize_out.inner.as_entire_buffer_binding(),
            },
        );
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
                original.set(&mut pass);
                out_bg.set(&mut pass);
                n_times = plane_fitter.run_mean_subtract(&mut pass, &plane_fitter_buffers);
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
