use std::sync::{Arc, OnceLock};

use glam::Affine2;
use itertools::{Itertools, chain, izip};
pub use shaders::plane_fit::NormalizeData;
use tracing::info;
use wgpu::{
    BufferUsages, CommandEncoder, ComputePass, ComputePipeline, Device, Extent3d, QuerySet,
    QueryType, Queue, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, util::align_to, wgt::QuerySetDescriptor,
};

use crate::{
    buffers::{StorageBuffer, TransformBuffer},
    shaders::{self, scan_image::NormalizeControl},
};

#[derive(Debug, Clone, Copy)]
pub enum NormalizationType {
    MinMax,
    StdDev(f64),
}
impl From<NormalizationType> for NormalizeControl {
    fn from(value: NormalizationType) -> Self {
        NormalizeControl {
            max_min: matches!(value, NormalizationType::MinMax) as u32,
            _pad: 0,
            std_dev_mul: match value {
                NormalizationType::MinMax => 0.,
                NormalizationType::StdDev(f) => f,
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteLinesError {
    #[error("line of length `{line_length}` is supposed to be `{correct_length}` to match image")]
    LineWrongLength {
        line_length: usize,
        correct_length: usize,
    },
    #[error("the image is already full")]
    ImageFull,
}

pub struct ImageComputeBuffers {
    size: [u32; 2],
    lines: u32,
    world_transform_buffer: TransformBuffer,
    image_size_buffer: StorageBuffer<u32>,
    image_data_buffer: StorageBuffer<f32>,
    planarize_buffer: StorageBuffer<f64>,
    normalize_buffer: StorageBuffer<shaders::plane_fit::NormalizeData>,
    normalize_control_buffer: StorageBuffer<shaders::scan_image::NormalizeControl>,
    image_src_bg: shaders::plane_fit::bind_groups::BindGroup0,
    normalize_bg: shaders::plane_fit::bind_groups::BindGroup1,
    pub(crate) scan_image_bg: shaders::scan_image::bind_groups::BindGroup1,
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
        let image_size_buffer = StorageBuffer::new(
            &device,
            size_buffer_label.as_deref(),
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            2,
            |buf| {
                buf[0] = size[0];
                buf[1] = lines;
            },
        );
        let data_buffer_label = label.map(|name| format!("{name}_data_buffer"));
        let image_data_buffer = StorageBuffer::new(
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
        let normalize_control_buffer = StorageBuffer::new(
            device,
            None,
            BufferUsages::UNIFORM | BufferUsages::COPY_SRC,
            1,
            |data| data[0] = NormalizationType::MinMax.into(),
        );
        let normalize_buffer = StorageBuffer::<shaders::plane_fit::NormalizeData>::new(
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
        let image_src_bg = shaders::plane_fit::bind_groups::BindGroup0::from_bindings(
            &device,
            shaders::plane_fit::bind_groups::BindGroupLayout0 {
                image_size: image_size_buffer.as_entire_buffer_binding(),
                image_in: image_data_buffer.as_entire_buffer_binding(),
            },
        );
        let scan_image_bg = shaders::scan_image::bind_groups::BindGroup1::from_bindings(
            device,
            shaders::scan_image::bind_groups::BindGroupLayout1 {
                quad2world: world_transform_buffer.as_entire_buffer_binding(),
                height_map: &image_texture.create_view(&TextureViewDescriptor::default()),
                normalize_data: normalize_buffer.as_entire_buffer_binding(),
                normalize_control: normalize_control_buffer.as_entire_buffer_binding(),
            },
        );
        let normalize_bg = shaders::plane_fit::bind_groups::BindGroup1::from_bindings(
            &device,
            shaders::plane_fit::bind_groups::BindGroupLayout1 {
                texture_out: &image_texture.create_view(&TextureViewDescriptor::default()),
                planarize_out: planarize_buffer.as_entire_buffer_binding(),
                normalize_out: normalize_buffer.as_entire_buffer_binding(),
            },
        );
        Self {
            size,
            lines,
            image_size_buffer,
            image_data_buffer,
            world_transform_buffer,
            planarize_buffer,
            normalize_buffer,
            normalize_control_buffer,
            scan_image_bg,
            image_src_bg,
            normalize_bg,
        }
    }
    pub fn write_world_transform(&self, queue: &Queue, transform: Affine2) {
        self.world_transform_buffer.write(queue, transform);
    }
    pub fn write_normalization_type(&self, queue: &Queue, normalization_type: NormalizationType) {
        self.normalize_control_buffer
            .queue_write(queue, 0, &[normalization_type.into()]);
    }
    pub fn write_line(&mut self, queue: &Queue, line: &[f32]) -> Result<(), WriteLinesError> {
        if self.lines == self.size[1] {
            return Err(WriteLinesError::ImageFull);
        }
        let correct_length = self.size[0] as usize;
        if line.len() != correct_length {
            return Err(WriteLinesError::LineWrongLength {
                line_length: line.len(),
                correct_length,
            });
        }
        self.image_data_buffer.queue_write(
            queue,
            self.lines as usize * self.size[0] as usize,
            line,
        );
        self.lines += 1;
        self.image_size_buffer.queue_write(queue, 1, &[self.lines]);
        Ok(())
    }
    pub fn current_size(&self) -> [u32; 2] {
        [self.size[0], self.lines]
    }
    pub fn download_normalize_data(
        &self,
        device: &Device,
        queue: &Queue,
    ) -> Arc<OnceLock<NormalizeData>> {
        self.normalize_buffer
            .queue_download_with(device, queue, ..1, |d| d[0])
    }
    pub fn download_planarize_data(
        &self,
        device: &Device,
        queue: &Queue,
        len: usize,
    ) -> Arc<OnceLock<Box<[f64]>>> {
        self.planarize_buffer.queue_download(device, queue, ..len)
    }
}

struct ScratchBuffers {
    xz: StorageBuffer<f64>,
    yz: StorageBuffer<f64>,
    std_dev: StorageBuffer<f64>,
    bg: shaders::plane_fit::bind_groups::BindGroup2,
    size: [u32; 2],
}
impl ScratchBuffers {
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
            bg: shaders::plane_fit::bind_groups::BindGroup2::from_bindings(
                &device,
                shaders::plane_fit::bind_groups::BindGroupLayout2 {
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
    scratch_buffers: ScratchBuffers,
}
impl ImageComputePipeline {
    pub fn new(device: &Device) -> Self {
        let n_timings = 5;
        Self {
            copy_image: shaders::plane_fit::compute::create_copy_image_pipeline(device),
            copy_image_transpose: shaders::plane_fit::compute::create_copy_image_transpose_pipeline(
                device,
            ),
            generate_sums_plane: shaders::plane_fit::compute::create_generate_sums_plane_pipeline(
                device,
            ),
            generate_sums_lines: shaders::plane_fit::compute::create_generate_sums_lines_pipeline(
                device,
            ),
            reduce_image: shaders::plane_fit::compute::create_reduce_image_pipeline(device),
            reduce_image_lines: shaders::plane_fit::compute::create_reduce_image_lines_pipeline(
                device,
            ),
            reduce_sums_plane: shaders::plane_fit::compute::create_reduce_sums_plane_pipeline(
                device,
            ),
            reduce_sums_lines: shaders::plane_fit::compute::create_reduce_sums_lines_pipeline(
                device,
            ),
            reduce_normalizations:
                shaders::plane_fit::compute::create_reduce_normalizations_pipeline(device),
            generate_normalization__mean_subtract:
                shaders::plane_fit::compute::create_generate_normalization__mean_subtract_pipeline(
                    device,
                ),
            write__mean_subtract: shaders::plane_fit::compute::create_write__mean_subtract_pipeline(
                device,
            ),
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
            scratch_buffers: ScratchBuffers::new(device, [1024, 1024]),
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
            self.scratch_buffers = ScratchBuffers::new(device, image.size);
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
        align_to(size[0] * size[1], shaders::plane_fit::WGS) / shaders::plane_fit::WGS,
        1,
        1,
    );
}

fn dispatch_reduction(pass: &mut ComputePass, size: [u32; 2]) {
    let mut remaining_data = size[0] * size[1];
    while remaining_data > 1 {
        let num_workgroups =
            align_to(remaining_data, shaders::plane_fit::WGS) / shaders::plane_fit::WGS;
        pass.dispatch_workgroups(num_workgroups, 1, 1);
        remaining_data = num_workgroups;
    }
}

fn dispatch_y_reduction(pass: &mut ComputePass, size: [u32; 2]) {
    let mut remaining_data = size[0];
    while remaining_data > 1 {
        let num_workgroups = align_to(remaining_data, shaders::plane_fit::WGS_SQUARE)
            / shaders::plane_fit::WGS_SQUARE;
        pass.dispatch_workgroups(
            align_to(size[1], shaders::plane_fit::WGS_SQUARE) / shaders::plane_fit::WGS_SQUARE,
            num_workgroups,
            1,
        );
        remaining_data = num_workgroups;
    }
}
