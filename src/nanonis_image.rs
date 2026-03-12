use glam::Affine2;
use image_compute::image_compute::FitType;
use nanonis_tcp::{commands::scan::FrameDataGrabResponse, scan_watcher::ScanWatcher, ScanDir};

use crate::{
    app::ImageMenu,
    scan_view::{ImageEncoder, ScanImage},
};

pub struct NanonisImage {
    pub image_data: ScanImage,
    scan_watcher: ScanWatcher<WatcherCallback>,
    nanonis: nanonis_tcp::blocking::NanonisTcp,
    fit_type: FitType,
    event_rx: std::sync::mpsc::Receiver<Event>,
    base_transform: Affine2,
}
impl NanonisImage {
    pub fn new(ctx: egui::Context, image_encoder: ImageEncoder) -> Self {
        let mut nanonis = nanonis_tcp::blocking::NanonisTcp::new("localhost:6503").unwrap();
        let buffer = nanonis.scan_buffer_get().unwrap();
        let size = [buffer.px_per_line as u32, buffer.num_lines as u32];
        let frame = nanonis.scan_frame_get().unwrap();
        let mut frame_data = nanonis
            .scan_frame_data_grab(30, nanonis_tcp::LineDir::Forward)
            .unwrap();
        if frame_data.scan_dir == ScanDir::Up {
            flip_buf(&mut frame_data.scan_data.data, frame_data.scan_data.size[1]);
        }
        let lines = nanonis.scan_wait_end_of_line(None).unwrap();
        let base_transform = Affine2::from_scale_angle_translation(
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
        let transform = match frame_data.scan_dir {
            ScanDir::Down => flip_transform(base_transform),
            ScanDir::Up => base_transform,
        };
        let image = ScanImage::new(
            &image_encoder,
            size,
            lines.line_number as u32,
            transform,
            |buf| {
                buf.copy_from_slice(&frame_data.scan_data.data);
            },
        );
        let fit_type = FitType::PlaneFitSubtract;
        image.write_texture(&image_encoder, fit_type);
        Self {
            image_data: image,
            scan_watcher,
            nanonis,
            fit_type,
            event_rx,
            base_transform,
        }
    }
    pub fn update(&mut self, encoder: &mut ImageEncoder) {
        let frame_meta = self.nanonis.scan_frame_get().unwrap();
        self.base_transform = Affine2::from_scale_angle_translation(
            [frame_meta.width * 1e9, frame_meta.height * -1e9].into(),
            frame_meta.angle / 180. * -3.14,
            [frame_meta.center_x * 1e9, frame_meta.center_y * 1e9].into(),
        );
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                Event::Frame {
                    num_lines,
                    mut frame,
                } => {
                    let new_size = [
                        frame.scan_data.size[1] as u32,
                        frame.scan_data.size[0] as u32,
                    ];
                    let transform = match frame.scan_dir {
                        ScanDir::Down => flip_transform(self.base_transform),
                        ScanDir::Up => self.base_transform,
                    };
                    if self.image_data.capacity() != new_size {
                        self.image_data = ScanImage::new(encoder, new_size, 0, transform, |_| {})
                    } else {
                        self.image_data.transform = transform;
                        if frame.scan_dir == ScanDir::Up {
                            flip_buf(&mut frame.scan_data.data, frame.scan_data.size[1]);
                        }
                        let [width, current_lines] = self.image_data.current_size();
                        let new_lines = num_lines - current_lines as usize;
                        if new_lines == 0 {
                            continue;
                        }
                        let src = &frame.scan_data.data[(current_lines * width) as usize..]
                            [..new_lines * width as usize];
                        self.image_data
                            .write_lines(encoder, new_lines, |buf| buf.copy_from_slice(src))
                            .unwrap()
                    }
                }
                Event::Clear => self.image_data.clear(encoder),
            }
        }
        self.image_data.write_texture(encoder, self.fit_type);
    }
}

fn flip_buf(buf: &mut [f32], width: usize) {
    buf.reverse();
    for r in 0..(buf.len() / width) {
        buf[r * width..][..width].reverse();
    }
}

pub fn flip_transform(t: Affine2) -> Affine2 {
    let flip = glam::Mat2::from_cols(glam::Vec2::new(1.0, 0.0), glam::Vec2::new(0.0, -1.0));
    Affine2 {
        matrix2: t.matrix2 * flip,
        translation: t.translation,
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
}
