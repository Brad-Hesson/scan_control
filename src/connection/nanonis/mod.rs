mod channel_state;
mod frame_worker;
mod line_worker;
mod status_worker;

use std::thread::JoinHandle;

use glam::DAffine2;
use itertools::izip;
use nanonis_tcp::{
    blocking::{self, NanonisTcp},
    error::{NanonisTcpError, NanonisTcpResult},
    LineDir, ScanDir,
};

use crate::{
    connection::{
        live_image::{FrameData, LiveImage},
        nanonis::{
            channel_state::ChannelState, frame_worker::FrameWorker, line_worker::LineWorker,
            status_worker::StatusWorker,
        },
        scan_area::ScanArea,
        shared_state::SharedState,
    },
    scan_view::ImageEncoder,
};

pub struct NanonisConnection {
    forward_data: SharedState<FrameData>,
    backward_data: SharedState<FrameData>,
    image_transform: SharedState<DAffine2>,
    area_transform: SharedState<DAffine2>,
    channel_state: SharedState<ChannelState>,
    scan_status: SharedState<ScanStatus>,
    frame_queue_tx: std::sync::mpsc::Sender<(LineDir, u32)>,
}

impl NanonisConnection {
    pub fn new(ctx: egui::Context, address: impl AsRef<str>) -> Self {
        let image_transform = SharedState::new_default();
        let area_transform = SharedState::new_default();
        let channel_state = SharedState::new_default();
        let forward_data = SharedState::new_default();
        let backward_data = SharedState::new_default();
        let scan_status = SharedState::new_default();
        let (frame_queue_tx, frame_queue_rx) = std::sync::mpsc::channel();
        let address = address.as_ref();
        LineWorker::new(&frame_queue_tx, &channel_state, &scan_status).run(address, 6501);
        FrameWorker::new(
            &ctx,
            &forward_data,
            &backward_data,
            frame_queue_rx,
            &scan_status,
        )
        .run(address, 6502);
        StatusWorker::new(
            &ctx,
            &image_transform,
            &area_transform,
            &channel_state,
            &scan_status,
        )
        .run(address, 6503);
        Self {
            image_transform,
            area_transform,
            channel_state,
            backward_data,
            forward_data,
            frame_queue_tx,
            scan_status,
        }
    }
    pub fn poll_connected(&mut self, encoder: &ImageEncoder) -> Option<ScanArea> {
        if self.image_transform.is_new() && self.area_transform.is_new() {
            Some(ScanArea::new(
                encoder,
                *self.area_transform.read(),
                *self.image_transform.read(),
            ))
        } else {
            None
        }
    }
    pub fn update_live_image(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) {
        self.update_channel(scan_area, encoder);
        self.update_area_transform(scan_area);
        self.update_image_transform(scan_area);
        self.update_image_data(&mut scan_area.live_image, encoder);
        self.update_scan_status(scan_area);
        if let Some(line_number) = self.scan_status.read_new().as_deref().copied() {
            println!("line number: {line_number:?}");
        }
    }
    pub fn update_scan_status(&mut self, scan_area: &mut ScanArea) {
        if let Some(scan_status) = self.scan_status.read_new().as_deref().copied() {
            scan_area.scan_status = scan_status;
        }
    }
    pub fn update_image_data(&mut self, live_image: &mut LiveImage, encoder: &ImageEncoder) {
        if let Some(forward_data) = self.forward_data.read_new() {
            live_image.forward_data = forward_data.clone();
            parking_lot::RwLockReadGuard::unlock_fair(forward_data);
            if live_image.line_dir == LineDir::Forward {
                live_image.write_and_update_texture(encoder);
            }
        }
        if let Some(backward_data) = self.backward_data.read_new() {
            live_image.backward_data = backward_data.clone();
            parking_lot::RwLockReadGuard::unlock_fair(backward_data);
            if live_image.line_dir == LineDir::Backward {
                live_image.write_and_update_texture(encoder);
            }
        }
    }
    pub fn update_image_transform(&mut self, scan_area: &mut ScanArea) {
        if let Some(new_transform) = self.image_transform.read_new().as_deref().copied() {
            scan_area.image_transform = new_transform;
            return;
        }
        self.image_transform.modify_conditional(
            |prev| *prev != scan_area.image_transform,
            |old| *old = scan_area.image_transform,
        );
    }
    pub fn update_area_transform(&mut self, scan_area: &mut ScanArea) {
        if let Some(new_transform) = self.area_transform.read_new().as_deref().copied() {
            scan_area.area_transform = new_transform;
        }
    }
    pub fn update_channel(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) {
        if let Some(state) = self.channel_state.read_new() {
            scan_area.channel_opts = state.channel_opts_names().collect();
            scan_area.channel_selected = state.selected_as_string();
            if let Some(ch) = state.selection {
                self.frame_queue_tx
                    .send((LineDir::Forward, ch as u32))
                    .unwrap();
                self.frame_queue_tx
                    .send((LineDir::Backward, ch as u32))
                    .unwrap();
            } else {
                scan_area.live_image.clear_texture(encoder);
            }
            return;
        }
        if self.channel_state.modify_conditional(
            |prev| prev.selected_as_string() != scan_area.channel_selected,
            |old| match &scan_area.channel_selected {
                Some(ch_name) => old.set_selection_by_name(&ch_name),
                None => old.selection = None,
            },
        ) {
            if let Some(ch) = self.channel_state.peek().selection {
                self.frame_queue_tx
                    .send((LineDir::Forward, ch as u32))
                    .unwrap();
                self.frame_queue_tx
                    .send((LineDir::Backward, ch as u32))
                    .unwrap();
            } else {
                scan_area.live_image.clear_texture(encoder);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScanStatus {
    pub scan_dir: ScanDir,
    pub line_number: u32,
    pub scanning: bool,
}
impl Default for ScanStatus {
    fn default() -> Self {
        Self {
            scan_dir: ScanDir::Down,
            line_number: Default::default(),
            scanning: false,
        }
    }
}
impl ScanStatus {
    pub fn position_float(&self, rows: u32) -> f64 {
        let mut pos = ((self.line_number as f64 - 0.5) / rows as f64) - 0.5;
        if self.scan_dir == ScanDir::Up{
            pos *= -1.;
        }
        pos
    }
}

trait Worker: Sized + Send + 'static {
    fn init(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()>;
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()>;
    fn run(mut self, addr: impl AsRef<str>, port: u16) -> JoinHandle<()> {
        let addr = addr.as_ref().to_string();
        std::thread::spawn(move || self.run_inner(addr, port))
    }
    fn run_inner(&mut self, addr: String, port: u16) {
        'reconnect: loop {
            let mut conn = loop {
                if let Ok(conn) = blocking::NanonisTcp::new((addr.as_str(), port)) {
                    break conn;
                }
            };
            'retry: loop {
                match self.init(&mut conn) {
                    Ok(_) => break,
                    Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Parse(_)) => {
                        continue 'retry;
                    }
                    Err(NanonisTcpError::Io(_)) => {
                        continue 'reconnect;
                    }
                }
            }
            'retry: loop {
                match self.work(&mut conn).inspect_err(|e| println!("{e}")) {
                    Ok(_) | Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Parse(_)) => {
                        continue 'retry;
                    }
                    Err(NanonisTcpError::Io(_)) => {
                        continue 'reconnect;
                    }
                }
            }
        }
    }
}
