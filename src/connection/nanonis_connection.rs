use core::f32;

use glam::Affine2;
use itertools::{izip, Itertools};
use nanonis_tcp::{
    blocking,
    commands::scan::{FrameDataGrabResponse, FrameGetResponse},
    nonblocking, LineDir, ScanDir, ScanMovementType,
};
use tokio::{net::ToSocketAddrs, sync::watch};

use crate::{
    connection::live_image::{Channel, LiveImage},
    scan_view::{static_image::StaticImage, ImageEncoder},
};

pub struct NanonisConnection {
    conn: blocking::NanonisTcp,
    pub live_image: LiveImage,
    stamp_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
    frame_forward_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
    frame_backward_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
    scanning_rx: watch::Receiver<bool>,
    transform_rx: watch::Receiver<Affine2>,
    size_rx: watch::Receiver<[u32; 2]>,
    name_rx: watch::Receiver<String>,
    signal_names_rx: watch::Receiver<Vec<String>>,
    channels_rx: watch::Receiver<Vec<usize>>,
    channel_tx: watch::Sender<Channel>,
}

impl NanonisConnection {
    pub fn new(ctx: egui::Context, image_encoder: &ImageEncoder, address: impl AsRef<str>) -> Self {
        let mut conn = blocking::NanonisTcp::new((address.as_ref().to_string(), 6501)).unwrap();
        let frame = conn.scan_frame_get().unwrap();
        let buffer = conn.scan_buffer_get().unwrap();
        let props = conn.scan_props_get().unwrap();
        let sig_names_resp = conn.signals_names_get().unwrap();
        let sig_names = sig_names_resp.names;
        let transform = transform_from_frame(&frame);
        let size = [buffer.px_per_line as u32, buffer.num_lines as u32];
        let name = props.series_name;
        let channels = buffer
            .channel_indexes
            .into_iter()
            .map(|v| v as usize)
            .collect_vec();
        let channel = channels
            .contains(&30)
            .then_some(Channel::Channel(30))
            .or(channels.get(0).copied().map(Channel::Channel))
            .unwrap_or(Channel::None);

        let (stamp_tx, stamp_rx) = watch::channel(None);
        let (frame_forward_tx, frame_forward_rx) = watch::channel(None);
        let (frame_backward_tx, frame_backward_rx) = watch::channel(None);
        let (scanning_tx, scanning_rx) = watch::channel(true);
        let (transform_tx, transform_rx) = watch::channel(transform);
        let (size_tx, size_rx) = watch::channel(size);
        let (name_tx, name_rx) = watch::channel(name.clone());
        let (signal_names_tx, signal_names_rx) = watch::channel(sig_names.clone());
        let (channels_tx, channels_rx) = watch::channel(channels.clone());
        let (channel_tx, channel_rx) = watch::channel(channel);

        tokio::spawn(scan_worker(
            (address.as_ref().to_string(), 6502),
            ctx.clone(),
            stamp_tx,
            scanning_tx.clone(),
            channel_rx.clone(),
            frame_forward_rx.clone(),
            frame_backward_rx.clone(),
        ));
        tokio::spawn(line_worker(
            (address.as_ref().to_string(), 6503),
            ctx.clone(),
            scanning_tx,
            frame_forward_tx,
            frame_backward_tx,
            channel_rx,
        ));
        tokio::spawn(status_worker(
            (address.as_ref().to_string(), 6504),
            ctx.clone(),
            transform_tx,
            size_tx,
            name_tx,
            signal_names_tx,
            channels_tx,
        ));

        let mut live_image = LiveImage::new(image_encoder, size, transform);
        live_image.channel_opts = channels;
        live_image.signal_names = sig_names;
        live_image.name = name;
        live_image.channel = channel;

        Self {
            size_rx,
            transform_rx,
            stamp_rx,
            frame_forward_rx,
            frame_backward_rx,
            live_image,
            scanning_rx,
            conn,
            name_rx,
            signal_names_rx,
            channels_rx,
            channel_tx,
        }
    }
    pub fn update(&mut self, encoder: &ImageEncoder) -> Option<StaticImage> {
        // update watchers from local vars
        self.channel_tx.send_if_modified(|prev| {
            if *prev != self.live_image.channel {
                *prev = self.live_image.channel;
                true
            } else {
                false
            }
        });
        // check updates on watch vars
        if self.channels_rx.has_changed().unwrap() {
            let channels = self.channels_rx.borrow_and_update().clone();
            self.live_image.channel_opts = channels;
        }
        if self.signal_names_rx.has_changed().unwrap() {
            let signal_names = self.signal_names_rx.borrow_and_update().clone();
            self.live_image.signal_names = signal_names;
        }
        if self.size_rx.has_changed().unwrap() {
            let new_size = *self.size_rx.borrow_and_update();
            if self.live_image.size() != new_size {
                self.live_image.resize(encoder, new_size);
            }
        }
        if self.name_rx.has_changed().unwrap() {
            let name = self.name_rx.borrow_and_update();
            self.live_image.name = name.clone();
        }
        if self.transform_rx.has_changed().unwrap() {
            self.live_image.transform = *self.transform_rx.borrow_and_update();
        }
        // Check for new live frames
        if self.frame_forward_rx.has_changed().unwrap() {
            if let Some(frame) = self.frame_forward_rx.borrow_and_update().as_ref() {
                self.live_image
                    .write_lines_forward(encoder, .., |buf| {
                        buf.copy_from_slice(&frame.scan_data.data)
                    })
                    .unwrap();
                self.live_image.update_texture_forward(encoder);
            }
        }
        if self.frame_backward_rx.has_changed().unwrap() {
            if let Some(frame) = self.frame_backward_rx.borrow_and_update().as_ref() {
                self.live_image
                    .write_lines_backward(encoder, .., |buf| {
                        buf.copy_from_slice(&frame.scan_data.data)
                    })
                    .unwrap();
                self.live_image.update_texture_backward(encoder);
            }
        }

        // Check for full frame stamps
        let mut stamp = None;
        if self.stamp_rx.has_changed().unwrap() {
            if let Some(frame) = self.stamp_rx.borrow_and_update().as_ref() {
                let size = [
                    frame.scan_data.size[0] as u32,
                    frame.scan_data.size[1] as u32,
                ];
                stamp = Some(self.live_image.stamp(encoder, size, |buf| {
                    buf.copy_from_slice(&frame.scan_data.data)
                }))
            }
        }
        stamp
    }
}

async fn scan_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    stamp_tx: watch::Sender<Option<FrameDataGrabResponse>>,
    scanning: watch::Sender<bool>,
    channel_rx: watch::Receiver<Channel>,
    frame_forward_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
    frame_backward_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
) {
    let mut conn = nonblocking::NanonisTcp::new(addr).await.unwrap();
    loop {
        conn.scan_wait_end_of_scan(None).await.unwrap();
        if !*scanning.borrow() {
            continue;
        }
        scanning.send_replace(false);
        if !channel_rx.borrow().is_some() {
            continue;
        }
        let frame_forward_borrow = frame_forward_rx.borrow();
        let Some(frame) = frame_forward_borrow.as_ref() else {
            continue;
        };
        if frame_early_exited(frame) {
            continue;
        }
        stamp_tx.send_replace(Some(frame.clone()));
        ctx.request_repaint();
    }
}

fn frame_early_exited(frame: &FrameDataGrabResponse) -> bool {
    let [height, width] = frame.scan_data.size;
    let lines = frame.scan_data.data.iter().step_by(width);
    let trailing_nans = match frame.scan_dir {
        ScanDir::Down => lines.rev().take_while(|v| v.is_nan()).count(),
        ScanDir::Up => lines.take_while(|v| v.is_nan()).count(),
    };
    let num_lines = height - trailing_nans;
    let min_lines = height / 4;
    num_lines < min_lines
}

async fn line_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    scanning: watch::Sender<bool>,
    frame_forward_tx: watch::Sender<Option<FrameDataGrabResponse>>,
    frame_backward_tx: watch::Sender<Option<FrameDataGrabResponse>>,
    channel_rx: watch::Receiver<Channel>,
) {
    let mut conn = nonblocking::NanonisTcp::new(addr).await.unwrap();
    let mut wanted_dir = LineDir::Forward;
    loop {
        let resp = conn.scan_wait_end_of_line(None).await.unwrap();
        let ScanMovementType::Scan(line_dir) = resp.movement_type else {
            continue;
        };
        scanning.send_replace(true);
        if line_dir != wanted_dir {
            continue;
        }
        toggle_dir(&mut wanted_dir);
        let Channel::Channel(ch) = *channel_rx.borrow() else {
            continue;
        };
        let frame = conn
            .scan_frame_data_grab(ch as u32, line_dir)
            .await
            .unwrap();
        match line_dir {
            LineDir::Forward => frame_forward_tx.send_replace(Some(frame)),
            LineDir::Backward => frame_backward_tx.send_replace(Some(frame)),
        };
        ctx.request_repaint();
    }
}

fn toggle_dir(line_dir: &mut LineDir) {
    match line_dir {
        LineDir::Forward => *line_dir = LineDir::Backward,
        LineDir::Backward => *line_dir = LineDir::Forward,
    }
}

async fn status_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    transform_tx: watch::Sender<Affine2>,
    size_tx: watch::Sender<[u32; 2]>,
    name_tx: watch::Sender<String>,
    signal_names_tx: watch::Sender<Vec<String>>,
    channels_tx: watch::Sender<Vec<usize>>,
) {
    let mut conn = nonblocking::NanonisTcp::new(addr).await.unwrap();
    loop {
        let frame = conn.scan_frame_get().await.unwrap();
        let transform = transform_from_frame(&frame);
        transform_tx.send_if_modified(|prev| {
            if izip!(prev.to_cols_array(), transform.to_cols_array()).any(|(a, b)| a != b) {
                *prev = transform;
                ctx.request_repaint();
                true
            } else {
                false
            }
        });
        let buffer = conn.scan_buffer_get().await.unwrap();
        let size = [buffer.px_per_line as u32, buffer.num_lines as u32];
        size_tx.send_if_modified(|prev| {
            if *prev != size {
                *prev = size;
                ctx.request_repaint();
                true
            } else {
                false
            }
        });
        let channels = buffer
            .channel_indexes
            .into_iter()
            .map(|v| v as usize)
            .collect_vec();
        channels_tx.send_if_modified(|prev| {
            if prev.as_slice() != channels.as_slice() {
                *prev = channels;
                ctx.request_repaint();
                true
            } else {
                false
            }
        });
        let props = conn.scan_props_get().await.unwrap();
        let name = props.series_name;
        name_tx.send_if_modified(|prev| {
            if *prev != name {
                *prev = name;
                ctx.request_repaint();
                true
            } else {
                false
            }
        });
        let sig_names_resp = conn.signals_names_get().await.unwrap();
        let sig_names = sig_names_resp.names;
        signal_names_tx.send_if_modified(|prev| {
            if prev.as_slice() != sig_names.as_slice() {
                *prev = sig_names;
                ctx.request_repaint();
                true
            } else {
                false
            }
        });
    }
}

fn transform_from_frame(frame: &FrameGetResponse) -> Affine2 {
    Affine2::from_scale_angle_translation(
        [frame.width * 1e9, frame.height * -1e9].into(),
        frame.angle / 180. * -3.14,
        [frame.center_x * 1e9, frame.center_y * 1e9].into(),
    )
}

#[test]
fn vec_eq() {
    let a = vec!["a", "b", "c"];
    let b = vec!["a", "c", "c"];
    dbg!(a == b);
}
