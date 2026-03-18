use core::f32;

use glam::Affine2;
use itertools::{izip, Itertools};
use nanonis_tcp::{
    blocking,
    commands::scan::{FrameDataGrabResponse, FrameGetResponse},
    nonblocking, LineDir, ScanMovementType,
};
use tokio::{net::ToSocketAddrs, sync::watch};

use crate::scan_view::{ImageEncoder, StaticImage};

pub struct NanonisConnection {
    conn: blocking::NanonisTcp,
    pub live_image: StaticImage,
    stamp_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
    frame_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
    scanning_rx: watch::Receiver<bool>,
    transform_rx: watch::Receiver<Affine2>,
    size_rx: watch::Receiver<[u32; 2]>,
    name_rx: watch::Receiver<String>,
    signal_names_rx: watch::Receiver<Vec<String>>,
    channels_rx: watch::Receiver<Vec<usize>>,
    channel_tx: watch::Sender<Option<usize>>,
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
            .then_some(30)
            .or(channels.get(0).copied());

        let (stamp_tx, stamp_rx) = watch::channel(None);
        let (frame_tx, frame_rx) = watch::channel(None);
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
            frame_rx.clone(),
        ));
        tokio::spawn(line_worker(
            (address.as_ref().to_string(), 6503),
            ctx.clone(),
            scanning_tx,
            frame_tx,
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

        let mut live_image =
            StaticImage::new(image_encoder, size, transform, |buf| buf.fill(f32::NAN));
        live_image.channels = channels;
        live_image.signal_names = sig_names;
        live_image.name = name;
        live_image.channel = channel;

        Self {
            size_rx,
            transform_rx,
            stamp_rx,
            frame_rx,
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
            self.live_image.channels = channels;
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
        if self.frame_rx.has_changed().unwrap() {
            if let Some(frame) = self.frame_rx.borrow_and_update().as_ref() {
                self.live_image
                    .write_lines(encoder, .., |buf| {
                        buf.copy_from_slice(&frame.scan_data.data)
                    })
                    .unwrap();
                self.live_image.update_texture(encoder);
            }
        }

        // Check for full frame stamps
        let mut stamp = None;
        if self.stamp_rx.has_changed().unwrap() {
            if let Some(frame) = self.stamp_rx.borrow_and_update().as_ref() {
                self.live_image.clear_texture(encoder);
                let mut image = StaticImage::new(
                    encoder,
                    self.live_image.size(),
                    self.live_image.transform,
                    |buf| buf.copy_from_slice(&frame.scan_data.data),
                );
                image.name = self.live_image.name.clone();
                image.norm_type = self.live_image.norm_type;
                image.std_dev = self.live_image.std_dev;
                image.fit_type = self.live_image.fit_type;
                image.update_texture(encoder);
                image.channel = self.live_image.channel;
                image.signal_names = self.live_image.signal_names.clone();
                image.channels = vec![self.live_image.channel.unwrap()];
                stamp = Some(image);
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
    channel_rx: watch::Receiver<Option<usize>>,
    frame_rx: watch::Receiver<Option<FrameDataGrabResponse>>,
) {
    let mut conn = nonblocking::NanonisTcp::new(addr).await.unwrap();
    loop {
        conn.scan_wait_end_of_scan(None).await.unwrap();
        if *scanning.borrow() && channel_rx.borrow().is_some() {
            scanning.send_replace(false);
            if let Some(frame) = frame_rx.borrow().as_ref() {
                if !frame_early_exited(frame) {
                    stamp_tx.send_replace(Some(frame.clone()));
                    ctx.request_repaint();
                }
            }
        }
    }
}

fn frame_early_exited(frame: &FrameDataGrabResponse) -> bool {
    let trailing_nans = match frame.scan_dir {
        nanonis_tcp::ScanDir::Down => frame
            .scan_data
            .data
            .iter()
            .step_by(frame.scan_data.size[1])
            .rev()
            .take_while(|v| v.is_nan())
            .count(),
        nanonis_tcp::ScanDir::Up => frame
            .scan_data
            .data
            .iter()
            .step_by(frame.scan_data.size[1])
            .take_while(|v| v.is_nan())
            .count(),
    };
    trailing_nans > frame.scan_data.size[0] * 3 / 4
}

async fn line_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    scanning: watch::Sender<bool>,
    frame_tx: watch::Sender<Option<FrameDataGrabResponse>>,
    channel_rx: watch::Receiver<Option<usize>>,
) {
    let mut conn = nonblocking::NanonisTcp::new(addr).await.unwrap();
    loop {
        let resp = conn.scan_wait_end_of_line(None).await.unwrap();
        match resp.movement_type {
            ScanMovementType::Scan(LineDir::Forward) => {
                if channel_rx.borrow().is_some() {
                    scanning.send_replace(true);
                    let channel = channel_rx.borrow().unwrap() as u32;
                    let frame = conn
                        .scan_frame_data_grab(channel, LineDir::Forward)
                        .await
                        .unwrap();
                    frame_tx.send_replace(Some(frame));
                    ctx.request_repaint();
                }
            }
            ScanMovementType::StartOfScan => {
                scanning.send_replace(true);
                ctx.request_repaint();
            }
            _ => {}
        }
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
