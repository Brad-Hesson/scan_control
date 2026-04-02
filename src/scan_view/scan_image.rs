use std::{ops::RangeBounds, sync::Arc};

use eframe::egui_wgpu;
use egui::{mutex::RwLock, Id, Pos2, Rect, Response, Sense, Ui};
use glam::{Affine2, DAffine2};
use image_compute::{
    buffers::BufferOpError,
    image_compute::{FitData, FitType, ImageComputeBuffers, NormalizationType, NormalizeData},
};
use uuid::Uuid;

use crate::scan_view::{callbacks::ImageCallback, view::ScanViewCtx, ImageEncoder};

pub struct ScanViewImage {
    uuid: Uuid,
    pub transform: DAffine2,
    pub norm_type: NormalizationType,
    image_buffers: ImageComputeBuffers,
    pub fit_data: Arc<RwLock<Option<FitData>>>,
    pub norm_data: Arc<RwLock<Option<NormalizeData>>>,
}
impl ScanViewImage {
    pub fn new(
        image_encoder: &ImageEncoder,
        size: [u32; 2],
        transform: DAffine2,
        norm_type: NormalizationType,
        init_fn: impl FnOnce(&mut [f32]),
    ) -> Self {
        let image_buffers = ImageComputeBuffers::new(
            &image_encoder.wgpu_state.device,
            &image_encoder.wgpu_state.queue,
            None,
            size,
            init_fn,
        );
        Self {
            uuid: Uuid::new_v4(),
            transform,
            image_buffers,
            norm_type,
            fit_data: Arc::new(RwLock::new(None)),
            norm_data: Arc::new(RwLock::new(None)),
        }
    }
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
    pub fn show(&self, ui: &mut Ui) -> Response {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let resp = ui
            .input(|i| i.pointer.latest_pos())
            .and_then(|pos| {
                let pos = glam::Vec2::new(pos.x, pos.y);
                let [x, y] = (ctx.world2egui() * self.transform)
                    .inverse()
                    .transform_point2(pos.into())
                    .abs()
                    .into();
                (x < 0.5 && y < 0.5).then(|| {
                    ui.interact(
                        ctx.rect,
                        egui::Id::new(self.uuid),
                        Sense::focusable_noninteractive() | Sense::click(),
                    )
                })
            })
            .unwrap_or_else(|| neutral_response(ui, egui::Id::new(self.uuid)));
        let callback = egui_wgpu::Callback::new_paint_callback(
            ctx.rect,
            ImageCallback {
                norm_type: self.norm_type,
                transform: self.transform,
                image_buffers: self.image_buffers.clone(),
            },
        );
        ui.painter().add(callback);
        resp
    }
    pub fn write_texture(&self, image_encoder: &ImageEncoder, fit_type: FitType) {
        let mut encoder = image_encoder.wgpu_state.device.create_command_encoder(
            &wgpu::wgt::CommandEncoderDescriptor {
                label: Some("write texture"),
            },
        );
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_encoder.pipeline.dispatch(
                &image_encoder.wgpu_state.device,
                &mut pass,
                &self.image_buffers,
                fit_type,
            );
        }
        image_encoder.wgpu_state.queue.submit([encoder.finish()]);
        {
            let norm_data = self.norm_data.clone();
            let fit_data = self.fit_data.clone();
            self.image_buffers
                .download_normalize_data(
                    &image_encoder.wgpu_state.device,
                    &image_encoder.wgpu_state.queue,
                    move |data| *norm_data.write() = Some(data),
                )
                .ok();
            self.image_buffers
                .download_fit_data(
                    &image_encoder.wgpu_state.device,
                    &image_encoder.wgpu_state.queue,
                    fit_type,
                    move |data| *fit_data.write() = Some(data),
                )
                .ok();
        }
    }
    pub fn clear(&self, image_encoder: &ImageEncoder) {
        let mut encoder = image_encoder
            .wgpu_state
            .device
            .create_command_encoder(&wgpu::wgt::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_encoder.pipeline.dispatch_clear_texture(
                &image_encoder.wgpu_state.device,
                &mut pass,
                &self.image_buffers,
            );
        }
        image_encoder.wgpu_state.queue.submit([encoder.finish()]);
        *self.norm_data.write() = None;
        *self.fit_data.write() = None;
    }
    pub fn write_lines(
        &self,
        image_encoder: &ImageEncoder,
        lines: impl RangeBounds<u32>,
        callback: impl Fn(&mut [f32]),
    ) -> Result<(), BufferOpError> {
        self.image_buffers
            .write_lines_range(&image_encoder.wgpu_state.queue, lines, callback)
    }
    pub fn size(&self) -> [u32; 2] {
        self.image_buffers.size()
    }
}

fn neutral_response(ui: &egui::Ui, id: egui::Id) -> Response {
    ui.interact(
        Rect::from_center_size(Pos2::ZERO, egui::Vec2::ZERO),
        id,
        Sense::empty(),
    )
}
