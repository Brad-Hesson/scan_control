use glam::Affine2;
use image_compute::image_compute::{FitType, NormalizationType, NormalizeData};
use nanonis_tcp::{commands::scan::FrameDataGrabResponse, scan_watcher::ScanWatcher, ScanDir};

use crate::{
    components::image_menu::{ImageMenu, NormType},
    scan_view::{ImageEncoder, ScanImage},
};

pub struct NanonisImage {
    pub image_data: ScanImage,
    scan_watcher: ScanWatcher<WatcherCallback>,
    nanonis: nanonis_tcp::blocking::NanonisTcp,
    event_rx: std::sync::mpsc::Receiver<Event>,
    transform: Affine2,
    fit_type: FitType,
    norm_type: NormType,
    std_dev: f32,
}
impl NanonisImage {
    pub fn new(ctx: egui::Context, image_encoder: ImageEncoder) -> Self {
        let mut nanonis = nanonis_tcp::blocking::NanonisTcp::new("localhost:6503").unwrap();
        let buffer = nanonis.scan_buffer_get().unwrap();
        let size = [buffer.px_per_line as u32, buffer.num_lines as u32];
        let frame = nanonis.scan_frame_get().unwrap();
        let frame_data = nanonis
            .scan_frame_data_grab(30, nanonis_tcp::LineDir::Forward)
            .unwrap();
        let transform = Affine2::from_scale_angle_translation(
            [frame.width * 1e9, frame.height * -1e9].into(),
            frame.angle / 180. * -3.14,
            [frame.center_x * 1e9, frame.center_y * 1e9].into(),
        );
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let callback = WatcherCallback::new(ctx, event_tx);
        let mut scan_watcher = tokio::runtime::Handle::current()
            .block_on(ScanWatcher::new(
                "localhost:6501",
                "localhost:6502",
                callback,
            ))
            .unwrap();
        scan_watcher.set_channel(30);
        let image = ScanImage::new(
            &image_encoder,
            size,
            transform,
            NormalizationType::MinMax,
            |buf| {
                buf.copy_from_slice(&frame_data.scan_data.data);
            },
        );
        let fit_type = FitType::MeanSubtract;
        image.write_texture(&image_encoder, fit_type);
        let norm_type = NormType::FullScale;
        Self {
            image_data: image,
            scan_watcher,
            nanonis,
            fit_type,
            event_rx,
            transform,
            norm_type,
            std_dev: 1.5,
        }
    }
    pub fn update(&mut self, encoder: &mut ImageEncoder) {
        self.image_data.transform = self.transform;
        let new_norm_type = match self.norm_type {
            NormType::FullScale => NormalizationType::MinMax,
            NormType::StdDev => NormalizationType::StdDev(self.std_dev),
        };
        self.image_data.norm_type = new_norm_type;
        let frame_meta = self.nanonis.scan_frame_get().unwrap();
        self.transform = Affine2::from_scale_angle_translation(
            [frame_meta.width * 1e9, frame_meta.height * -1e9].into(),
            frame_meta.angle / 180. * -3.14,
            [frame_meta.center_x * 1e9, frame_meta.center_y * 1e9].into(),
        );
        let mut updated = false;
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                Event::Frame { frame, .. } => {
                    updated = true;
                    let new_size = [
                        frame.scan_data.size[1] as u32,
                        frame.scan_data.size[0] as u32,
                    ];
                    if self.image_data.size() != new_size {
                        self.image_data = ScanImage::new(
                            encoder,
                            new_size,
                            self.transform,
                            new_norm_type,
                            |buf| buf.copy_from_slice(&frame.scan_data.data),
                        )
                    } else {
                        self.image_data
                            .write_lines_range(encoder, .., |buf| {
                                buf.copy_from_slice(&frame.scan_data.data)
                            })
                            .unwrap();
                    }
                }
                Event::Clear => {
                    // self.image_data.clear(encoder)
                }
            }
        }
        if updated {
            println!("updated");
            self.image_data.write_texture(encoder, self.fit_type);
        }
    }
}

struct WatcherCallback {
    event_tx: std::sync::mpsc::Sender<Event>,
    ctx: egui::Context,
}
impl WatcherCallback {
    pub fn new(ctx: egui::Context, event_tx: std::sync::mpsc::Sender<Event>) -> Self {
        Self { event_tx, ctx }
    }
}
impl nanonis_tcp::scan_watcher::Callback for WatcherCallback {
    fn frame(
        &mut self,
        num_lines: usize,
        frame: nanonis_tcp::commands::scan::FrameDataGrabResponse,
    ) {
        self.event_tx
            .send(Event::Frame { num_lines, frame })
            .unwrap();
        self.ctx.request_repaint();
    }

    fn start(&mut self) {
        self.event_tx.send(Event::Clear).unwrap();
        self.ctx.request_repaint();
    }
}

pub enum Event {
    Frame {
        num_lines: usize,
        frame: FrameDataGrabResponse,
    },
    Clear,
}

impl ImageMenu for NanonisImage {
    fn fit_type_mut(&mut self) -> &mut FitType {
        &mut self.fit_type
    }

    fn image_data_mut(&mut self) -> &mut ScanImage {
        &mut self.image_data
    }

    fn norm_type_mut(&mut self) -> &mut NormType {
        &mut self.norm_type
    }

    fn std_dev_mut(&mut self) -> &mut f32 {
        &mut self.std_dev
    }
}
