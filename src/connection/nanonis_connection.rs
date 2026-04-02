use core::f32;
use std::{net::ToSocketAddrs, sync::Arc};

use glam::{Affine2, DAffine2};
use itertools::{izip, Itertools};
use nanonis_tcp::{blocking, commands::scan::FrameGetResponse, LineDir, ScanDir, ScanMovementType};

use crate::{
    connection::{
        backing::BufferState,
        live_image::{Channel, LiveImage},
        shared_state::SharedState,
    },
    scan_view::{static_image::StaticImage, ImageEncoder},
};

pub struct NanonisConnection {
    conn: blocking::NanonisTcp,
    pub live_image: LiveImage,
    stamp: SharedState<BufferState>,
    frame: SharedState<BufferState>,
    scanning: SharedState<bool>,
    transform: SharedState<DAffine2>,
    name: SharedState<String>,
    signal_names: SharedState<Vec<String>>,
    channel_opts: SharedState<Vec<usize>>,
    channel: SharedState<Channel>,
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
            .unwrap_or(Channel::Channel(0));
        let frame_data_forward = if let Channel::Channel(ch) = channel {
            conn.scan_frame_data_grab(ch as u32, LineDir::Forward)
                .unwrap()
        } else {
            panic!()
        };
        let frame_data_backward = if let Channel::Channel(ch) = channel {
            conn.scan_frame_data_grab(ch as u32, LineDir::Backward)
                .unwrap()
        } else {
            panic!()
        };
        let buffer_state = BufferState {
            size: [buffer.num_lines, buffer.px_per_line],
            buf_f: Arc::new(frame_data_forward.scan_data.data),
            buf_b: Arc::new(frame_data_backward.scan_data.data),
            scan_dir: ScanDir::Down,
        };

        let mut live_image = LiveImage::new(image_encoder, buffer_state.clone(), transform);
        live_image.channel_opts = channels.clone();
        live_image.signal_names = sig_names.clone();
        live_image.name = name.clone();
        live_image.channel = channel;

        let stamp = SharedState::new(buffer_state.clone());
        let frame = SharedState::new(buffer_state);
        let scanning = SharedState::new(true);
        let transform = SharedState::new(transform);
        let name = SharedState::new(name.clone());
        let signal_names = SharedState::new(sig_names.clone());
        let channel_opts = SharedState::new(channels.clone());
        let channel = SharedState::new(channel);

        macro_rules! clone {
            ($i:ident) => {
                let $i = $i.clone();
            };
        }
        {
            let addr = (address.as_ref().to_string(), 6502);
            clone!(ctx);
            clone!(stamp);
            clone!(scanning);
            clone!(channel);
            clone!(frame);
            std::thread::spawn(move || {
                scan_worker(addr, ctx, stamp, scanning, channel, frame);
            });
        }
        {
            let addr = (address.as_ref().to_string(), 6503);
            clone!(ctx);
            clone!(scanning);
            clone!(frame);
            clone!(channel);
            std::thread::spawn(move || {
                line_worker(addr, ctx, scanning, frame, channel);
            });
        }
        {
            let addr = (address.as_ref().to_string(), 6504);
            clone!(ctx);
            clone!(transform);
            clone!(name);
            clone!(signal_names);
            clone!(channel_opts);
            std::thread::spawn(move || {
                status_worker(addr, ctx, transform, name, signal_names, channel_opts)
            });
        }

        Self {
            transform,
            stamp,
            frame,
            live_image,
            scanning,
            conn,
            name,
            signal_names,
            channel_opts,
            channel,
        }
    }
    pub fn update(&mut self, encoder: &ImageEncoder) -> Option<StaticImage> {
        // update watchers from local vars
        self.channel.modify_conditional(
            |prev| *prev != self.live_image.channel,
            |val| *val = self.live_image.channel,
        );
        // check updates on watch vars
        if let Some(new_channel_opts) = self.channel_opts.read_new() {
            self.live_image.channel_opts = new_channel_opts.clone();
        }
        if let Some(new_signal_names) = self.signal_names.read_new() {
            self.live_image.signal_names = new_signal_names.clone();
        }
        if let Some(new_name) = self.name.read_new() {
            self.live_image.name = new_name.clone();
        }
        if let Some(transform) = self.transform.read_new() {
            self.live_image.transform = *transform;
        }
        // Check for new live frames
        if let Some(new_buffer_state) = self.frame.read_new() {
            let new_size = [
                new_buffer_state.size[1] as u32,
                new_buffer_state.size[0] as u32,
            ];
            if new_size != self.live_image.size() {
                self.live_image.resize(encoder, new_size);
            } else {
                self.live_image.buffers = new_buffer_state.clone();
                self.live_image.update_texture(encoder);
            }
        }

        // Check for full frame stamps
        if let Some(frame) = self.stamp.read_new().as_deref() {
            Some(self.live_image.stamp(encoder, frame.clone()))
        } else {
            None
        }
    }
}

fn scan_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    mut stamp: SharedState<BufferState>,
    mut scanning: SharedState<bool>,
    channel: SharedState<Channel>,
    buffer_state: SharedState<BufferState>,
) {
    let mut conn = blocking::NanonisTcp::new(addr).unwrap();
    loop {
        conn.scan_wait_end_of_scan(None).unwrap();
        if !*scanning.peek() {
            continue;
        }
        scanning.write(false);
        if !channel.peek().is_some() {
            continue;
        }
        let frame = buffer_state.peek().clone();
        if frame_early_exited(&frame) {
            continue;
        }
        stamp.write(frame);
        ctx.request_repaint();
    }
}

fn frame_early_exited(buffers: &BufferState) -> bool {
    let [height, width] = buffers.size;
    let lines = buffers.buf_f.iter().step_by(width);
    let trailing_nans = match buffers.scan_dir {
        ScanDir::Down => lines.rev().take_while(|v| v.is_nan()).count(),
        ScanDir::Up => lines.take_while(|v| v.is_nan()).count(),
    };
    let num_lines = height - trailing_nans;
    let min_lines = height / 4;
    num_lines < min_lines
}

fn line_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    mut scanning: SharedState<bool>,
    mut buffer_state: SharedState<BufferState>,
    channel: SharedState<Channel>,
) {
    let mut conn = blocking::NanonisTcp::new(addr).unwrap();
    let mut wanted_dir = LineDir::Forward;
    loop {
        let resp = conn.scan_wait_end_of_line(None).unwrap();
        let ScanMovementType::Scan(line_dir) = resp.movement_type else {
            continue;
        };
        scanning.write(true);
        if line_dir != wanted_dir {
            continue;
        }
        toggle_dir(&mut wanted_dir);
        let Channel::Channel(ch) = *channel.peek() else {
            continue;
        };
        let new_frame = conn.scan_frame_data_grab(ch as u32, line_dir).unwrap();
        let new_size = new_frame.scan_data.size;
        if !buffer_state.modify_conditional(
            |prev| prev.size != new_size,
            |val| {
                *val = BufferState {
                    size: new_size,
                    buf_f: Arc::new(vec![f32::NAN; new_size[0] * new_size[1]]),
                    buf_b: Arc::new(vec![f32::NAN; new_size[0] * new_size[1]]),
                    scan_dir: ScanDir::Down,
                }
            },
        ) {
            match line_dir {
                LineDir::Forward => buffer_state.modify(|val| {
                    val.buf_f = Arc::new(new_frame.scan_data.data);
                    val.scan_dir = new_frame.scan_dir;
                }),
                LineDir::Backward => buffer_state.modify(|val| {
                    val.buf_b = Arc::new(new_frame.scan_data.data);
                    val.scan_dir = new_frame.scan_dir;
                }),
            };
        }
        ctx.request_repaint();
    }
}

pub fn toggle_dir(line_dir: &mut LineDir) {
    match line_dir {
        LineDir::Forward => *line_dir = LineDir::Backward,
        LineDir::Backward => *line_dir = LineDir::Forward,
    }
}

fn status_worker(
    addr: impl ToSocketAddrs,
    ctx: egui::Context,
    mut transform: SharedState<DAffine2>,
    mut name: SharedState<String>,
    mut signal_names: SharedState<Vec<String>>,
    mut channel_opts: SharedState<Vec<usize>>,
) {
    let mut conn = blocking::NanonisTcp::new(addr).unwrap();
    loop {
        let frame = conn.scan_frame_get().unwrap();
        let new_transform = transform_from_frame(&frame);
        if transform.modify_conditional(
            |prev| izip!(prev.to_cols_array(), new_transform.to_cols_array()).any(|(a, b)| a != b),
            |val| *val = new_transform,
        ) {
            ctx.request_repaint();
        };
        let buffer = conn.scan_buffer_get().unwrap();
        let channels = buffer
            .channel_indexes
            .into_iter()
            .map(|v| v as usize)
            .collect_vec();
        if channel_opts.modify_conditional(|prev| *prev != channels, |val| *val = channels.clone())
        {
            ctx.request_repaint();
        }
        let props = conn.scan_props_get().unwrap();
        if name.modify_conditional(
            |prev| *prev != props.series_name,
            |val| *val = props.series_name.clone(),
        ) {
            ctx.request_repaint();
        };
        let sig_names_resp = conn.signals_names_get().unwrap();
        if signal_names.modify_conditional(
            |prev| *prev != sig_names_resp.names,
            |val| *val = sig_names_resp.names.clone(),
        ) {
            ctx.request_repaint();
        };
    }
}

fn transform_from_frame(frame: &FrameGetResponse) -> DAffine2 {
    DAffine2::from_scale_angle_translation(
        [frame.width as f64 * 1e9, frame.height as f64 * -1e9].into(),
        frame.angle as f64 / 180. * -3.14,
        [frame.center_x as f64 * 1e9, frame.center_y as f64 * 1e9].into(),
    )
}

#[test]
fn vec_eq() {
    let a = vec!["a", "b", "c"];
    let b = vec!["a", "c", "c"];
    dbg!(a == b);
}
