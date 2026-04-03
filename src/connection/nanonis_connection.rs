use core::f32;
use std::{net::ToSocketAddrs, sync::Arc, time::Duration};

use glam::{Affine2, DAffine2};
use itertools::{izip, Itertools};
use nanonis_tcp::{
    blocking::{self, NanonisTcp},
    commands::scan::FrameGetResponse,
    error::{NanonisTcpError, NanonisTcpResult},
    LineDir, ScanDir, ScanMovementType,
};

use crate::{
    connection::{
        backing::BufferState,
        live_image::{Channel, LiveImage},
        shared_state::{SharedState, Updating},
    },
    scan_view::{static_image::StaticImage, ImageEncoder},
};

pub struct NanonisConnection {
    forward_data: SharedState<Arc<Box<[f32]>>>,
    backward_data: SharedState<Arc<Box<[f32]>>>,
    size: SharedState<[u32; 2]>,
    scanning: SharedState<bool>,
    transform: SharedState<DAffine2>,
    name: SharedState<String>,
    signal_names: SharedState<Vec<String>>,
    channel_opts: SharedState<Vec<usize>>,
    channel: SharedState<Channel>,
}

impl NanonisConnection {
    pub fn new(ctx: egui::Context, address: impl AsRef<str>) -> Self {
        todo!("make each worker a struct that takes state, then implement helper methods");
        todo!("maybe make all state a single struct, and have helpers in it as well, then pass state struct to worker structs");
        todo!("go back to grouping states like channel stuff and frames");
        todo!("make a connection trait with methods like poll_connect, update, etc.");
        let transform = SharedState::new_default();
        let name = SharedState::new_default();
        let scanning = SharedState::new(false);
        let signal_names = SharedState::new_default();
        let channel_opts = SharedState::new_default();
        let channel = SharedState::new_default();
        let forward_data = SharedState::new_default();
        let backward_data = SharedState::new_default();
        let size = SharedState::new_default();

        macro_rules! clone {
            ($i:ident) => {
                let $i = $i.clone();
            };
            (mut $i:ident) => {
                let mut $i = $i.clone();
            };
        }
        // {
        //     let addr = (address.as_ref().to_string(), 6502);
        //     clone!(ctx);
        //     clone!(scanning);
        //     clone!(channel);
        //     clone!(frame);
        //     std::thread::spawn(move || {
        //         scan_worker(addr, ctx, stamp, scanning, channel, frame);
        //     });
        // }
        {
            let addr = (address.as_ref().to_string(), 6501);
            clone!(ctx);
            clone!(mut forward_data);
            clone!(mut size);
            clone!(mut channel);
            std::thread::spawn(move || 'reconnect: loop {
                let mut conn = loop {
                    if let Ok(conn) = blocking::NanonisTcp::new((addr.0.as_str(), addr.1)) {
                        break conn;
                    }
                };
                'retry: loop {
                    match line_worker(
                        LineDir::Forward,
                        &mut conn,
                        &ctx,
                        &mut forward_data,
                        &mut size,
                        &mut channel,
                    ) {
                        Ok(_) => unreachable!(),
                        Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Parse(_)) => {
                            continue 'retry;
                        }
                        Err(NanonisTcpError::Io(_)) => {
                            continue 'reconnect;
                        }
                    }
                }
            });
        }
        {
            let addr = (address.as_ref().to_string(), 6502);
            clone!(ctx);
            clone!(mut backward_data);
            clone!(mut size);
            clone!(mut channel);
            std::thread::spawn(move || 'reconnect: loop {
                let mut conn = loop {
                    if let Ok(conn) = blocking::NanonisTcp::new((addr.0.as_str(), addr.1)) {
                        break conn;
                    }
                };
                'retry: loop {
                    match line_worker(
                        LineDir::Backward,
                        &mut conn,
                        &ctx,
                        &mut backward_data,
                        &mut size,
                        &mut channel,
                    ) {
                        Ok(_) => unreachable!(),
                        Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Parse(_)) => {
                            continue 'retry;
                        }
                        Err(NanonisTcpError::Io(_)) => {
                            continue 'reconnect;
                        }
                    }
                }
            });
        }
        {
            let addr = (address.as_ref().to_string(), 6503);
            clone!(ctx);
            clone!(mut transform);
            clone!(mut name);
            clone!(mut signal_names);
            clone!(mut channel_opts);
            clone!(mut channel);
            std::thread::spawn(move || 'reconnect: loop {
                let mut conn = loop {
                    if let Ok(conn) = blocking::NanonisTcp::new((addr.0.as_str(), addr.1)) {
                        break conn;
                    }
                };
                println!("connected status worker");
                'retry: loop {
                    match status_worker(
                        &mut conn,
                        &ctx,
                        &mut transform,
                        &mut name,
                        &mut signal_names,
                        &mut channel_opts,
                        &mut channel,
                    ) {
                        Ok(_) => unreachable!(),
                        Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Parse(_)) => {
                            continue 'retry;
                        }
                        Err(NanonisTcpError::Io(_)) => {
                            continue 'reconnect;
                        }
                    }
                }
            });
        }

        Self {
            transform,
            name,
            signal_names,
            channel_opts,
            channel,
            backward_data,
            forward_data,
            size,
            scanning,
        }
    }
    pub fn update(
        &mut self,
        encoder: &ImageEncoder,
        live_image: &mut LiveImage,
    ) -> Option<StaticImage> {
        // update watchers from local vars
        self.channel.modify_conditional(
            |prev| *prev != live_image.channel,
            |val| *val = live_image.channel,
        );
        // check updates on watch vars
        if let Some(new_channel_opts) = self.channel_opts.read_new().as_deref().cloned() {
            live_image.channel_opts = new_channel_opts;
        }
        if let Some(new_signal_names) = self.signal_names.read_new().as_deref().cloned() {
            live_image.signal_names = new_signal_names;
        }
        if let Some(new_name) = self.name.read_new().as_deref().cloned() {
            live_image.name = new_name;
        }
        if let Some(transform) = self.transform.read_new().as_deref().copied() {
            live_image.transform = transform;
        }
        // Check for new live frames
        if let Some(forward_frame) = self.forward_data.read_new().as_deref().cloned() {
            live_image.forward_data = forward_frame.clone();
            live_image.update_texture(encoder);
        }
        if let Some(backward_frame) = self.backward_data.read_new().as_deref().cloned() {
            live_image.backward_data = backward_frame.clone();
            live_image.update_texture(encoder);
        }
        if let Some(new_size) = self.size.read_new().as_deref().copied() {
            if new_size != live_image.size() {
                live_image.resize(encoder, new_size);
            }
        }
        None

        // Check for full frame stamps
        // if let Some(frame) = self.stamp.read_new().as_deref() {
        //     Some(self.live_image.stamp(encoder, frame.clone()))
        // } else {
        //     None
        // }
    }
    pub fn poll_connected(&mut self, encoder: &ImageEncoder) -> Option<LiveImage> {
        println!("---- vars ----");
        if [
            &self.transform as &dyn Updating,
            &self.signal_names,
            &self.size,
            &self.forward_data,
            &self.backward_data,
            &self.channel_opts,
            &self.channel,
            &self.name,
        ]
        .into_iter()
        .map(|w| w.is_new())
        .inspect(|b| println!("{b:?}"))
        .all(|b| b)
        {
            let transform = *self.transform.read();
            let signal_names = self.signal_names.read().clone();
            let size = *self.size.read();
            let forward_data = self.forward_data.read().clone();
            let backward_data = self.backward_data.read().clone();
            let channel_opts = self.channel_opts.read().clone();
            let channel = self.channel.read().clone();
            let name = self.name.read().clone();
            Some(LiveImage::new(
                encoder,
                transform,
                size,
                forward_data,
                backward_data,
                signal_names,
                channel_opts,
                channel,
                name,
            ))
        } else {
            None
        }
    }
}

// fn scan_worker(
//     addr: impl ToSocketAddrs,
//     ctx: egui::Context,
//     mut stamp: SharedState<BufferState>,
//     mut scanning: SharedState<bool>,
//     channel: SharedState<Channel>,
//     buffer_state: SharedState<BufferState>,
// ) {
//     let mut conn = blocking::NanonisTcp::new(addr).unwrap();
//     loop {
//         conn.scan_wait_end_of_scan(None).unwrap();
//         if !*scanning.peek() {
//             continue;
//         }
//         scanning.write(false);
//         if !channel.peek().is_some() {
//             continue;
//         }
//         let frame = buffer_state.peek().clone();
//         if frame_early_exited(&frame) {
//             continue;
//         }
//         stamp.write(frame);
//         ctx.request_repaint();
//     }
// }

// fn frame_early_exited(buffers: &BufferState) -> bool {
//     let [height, width] = buffers.size;
//     let lines = buffers.buf_f.iter().step_by(width);
//     let trailing_nans = match buffers.scan_dir {
//         ScanDir::Down => lines.rev().take_while(|v| v.is_nan()).count(),
//         ScanDir::Up => lines.take_while(|v| v.is_nan()).count(),
//     };
//     let num_lines = height - trailing_nans;
//     let min_lines = height / 4;
//     num_lines < min_lines
// }

fn line_worker(
    line_dir: LineDir,
    conn: &mut NanonisTcp,
    ctx: &egui::Context,
    frame_state: &mut SharedState<Arc<Box<[f32]>>>,
    size_state: &mut SharedState<[u32; 2]>,
    channel_state: &mut SharedState<Channel>,
) -> NanonisTcpResult<()> {
    while !channel_state.is_new() {
        std::thread::sleep(Duration::from_millis(100));
    }
    let channel = *channel_state.read();
    println!("channel changed to {:?}", channel);
    match channel {
        Channel::None => {
            size_state.modify(|val| *val = [1, 1]);
            frame_state.modify(|val| *val = Arc::new(vec![f32::NAN].into_boxed_slice()));
        }
        Channel::Channel(ch) => {
            println!("grabbing frame");
            let new_frame = conn.scan_frame_data_grab(ch as u32, line_dir)?;
            println!("grabbed frame");
            let frame_data = Arc::new(new_frame.scan_data.data.into_boxed_slice());
            let size = [
                new_frame.scan_data.size[1] as u32,
                new_frame.scan_data.size[0] as u32,
            ];
            println!("setting data with size {size:?}");
            size_state.modify(|val| *val = size);
            frame_state.modify(|val| *val = frame_data);
        }
    }
    loop {
        let resp = conn.scan_wait_end_of_line(None)?;
        match resp.movement_type {
            ScanMovementType::Scan(dir) if dir == line_dir => {}
            _ => continue,
        };
        let Channel::Channel(ch) = *channel_state.peek() else {
            continue;
        };
        let new_frame = conn.scan_frame_data_grab(ch as u32, line_dir)?;
        let frame_data = Arc::new(new_frame.scan_data.data.into_boxed_slice());
        let size = [
            new_frame.scan_data.size[1] as u32,
            new_frame.scan_data.size[0] as u32,
        ];
        size_state.modify_conditional(|prev| *prev != size, |val| *val = size);
        frame_state.modify(|buf| *buf = frame_data);
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
    conn: &mut NanonisTcp,
    ctx: &egui::Context,
    transform: &mut SharedState<DAffine2>,
    name: &mut SharedState<String>,
    signal_names: &mut SharedState<Vec<String>>,
    channel_opts: &mut SharedState<Vec<usize>>,
    channel_state: &mut SharedState<Channel>,
) -> NanonisTcpResult<()> {
    loop {
        let frame = conn.scan_frame_get()?;
        let new_transform = transform_from_frame(&frame);
        if transform.modify_conditional(
            |prev| izip!(prev.to_cols_array(), new_transform.to_cols_array()).any(|(a, b)| a != b),
            |val| *val = new_transform,
        ) {
            ctx.request_repaint();
        };
        let buffer = conn.scan_buffer_get()?;
        let channels = buffer
            .channel_indexes
            .into_iter()
            .map(|v| v as usize)
            .collect_vec();
        if channel_opts.modify_conditional(|prev| *prev != channels, |val| *val = channels.clone())
        {
            let curr_channel = *channel_state.read();
            if !curr_channel
                .as_opt()
                .is_some_and(|ch| channels.contains(&ch))
            {
                if channels.contains(&30) {
                    channel_state.modify(|val| *val = Channel::Channel(30));
                } else if !channels.is_empty() {
                    channel_state.modify(|val| *val = Channel::Channel(channels[0]));
                } else {
                    channel_state.modify(|val| *val = Channel::None);
                }
            }
            ctx.request_repaint();
        }
        let props = conn.scan_props_get()?;
        if name.modify_conditional(
            |prev| *prev != props.series_name,
            |val| *val = props.series_name.clone(),
        ) {
            ctx.request_repaint();
        };
        let sig_names_resp = conn.signals_names_get()?;
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
