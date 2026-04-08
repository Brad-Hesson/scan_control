mod channel_state;
mod frame_worker;
mod line_worker;
mod status_worker;

use std::{sync::Arc, thread::JoinHandle, time::Duration};

use crossbeam::{
    queue::ArrayQueue,
    sync::{Parker, Unparker},
};
use glam::{DAffine2, DVec2};
use nanonis_tcp::{
    blocking::{self, NanonisTcp},
    error::{NanonisTcpError, NanonisTcpResult},
    LineDir, ScanDir,
};
use tracing::{error, info, instrument, warn, Level};

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
    area_size: SharedState<DVec2>,
    channel_state: SharedState<ChannelState>,
    scan_status: SharedState<ScanStatus>,
    frame_queue_tx: OverwriteQueueSender<(LineDir, u32)>,
    tip_pos: SharedState<DVec2>,
}

impl NanonisConnection {
    pub fn new(ctx: egui::Context, address: impl AsRef<str>) -> Self {
        let image_transform = SharedState::new_default();
        let area_size = SharedState::new_default();
        let channel_state = SharedState::new_default();
        let forward_data = SharedState::new_default();
        let backward_data = SharedState::new_default();
        let scan_status = SharedState::new_default();
        let tip_pos = SharedState::new_default();
        let (frame_queue_tx, frame_queue_rx) = overwrite_queue(2);
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
            &area_size,
            &channel_state,
            &scan_status,
            &tip_pos,
        )
        .run(address, 6503);
        Self {
            image_transform,
            area_size,
            channel_state,
            backward_data,
            forward_data,
            frame_queue_tx,
            scan_status,
            tip_pos,
        }
    }
    pub fn poll_connected(&mut self, encoder: &ImageEncoder) -> Option<ScanArea> {
        if self.image_transform.is_new() && self.area_size.is_new() && self.tip_pos.is_new() {
            Some(ScanArea::new(
                encoder,
                *self.area_size.read(),
                *self.image_transform.read(),
                *self.tip_pos.read(),
            ))
        } else {
            None
        }
    }
    pub fn update_live_image(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) {
        self.update_channel(scan_area, encoder);
        self.update_area_size(scan_area);
        self.update_image_transform(scan_area);
        self.update_image_data(&mut scan_area.live_image, encoder);
        self.update_scan_status(scan_area);
        self.update_tip_pos(scan_area);
    }
    fn update_tip_pos(&mut self, scan_area: &mut ScanArea) {
        if let Some(tip_pos) = self.tip_pos.read_new().as_deref().copied() {
            scan_area.tip_pos = tip_pos;
        }
    }
    fn update_scan_status(&mut self, scan_area: &mut ScanArea) {
        if let Some(scan_status) = self.scan_status.read_new().as_deref().copied() {
            scan_area.scan_status = scan_status;
        }
    }
    fn update_image_data(&mut self, live_image: &mut LiveImage, encoder: &ImageEncoder) {
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
    fn update_image_transform(&mut self, scan_area: &mut ScanArea) {
        if let Some(new_transform) = self.image_transform.read_new().as_deref().copied() {
            scan_area.image_transform = new_transform;
            return;
        }
        self.image_transform.modify_conditional(
            |prev| *prev != scan_area.image_transform,
            |old| *old = scan_area.image_transform,
        );
    }
    fn update_area_size(&mut self, scan_area: &mut ScanArea) {
        if let Some(new_size) = self.area_size.read_new().as_deref().copied() {
            scan_area.area_size = new_size;
        }
    }
    fn request_full_frame(tx: &OverwriteQueueSender<(LineDir, u32)>, channel: usize) {
        tx.send((LineDir::Forward, channel as u32));
        tx.send((LineDir::Backward, channel as u32));
    }
    fn update_channel(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) {
        if let Some(state) = self.channel_state.read_new() {
            scan_area.channel_opts = state.channel_opts_names().collect();
            scan_area.channel_selected = state.selected_as_string();
            if let Some(ch) = state.selection {
                Self::request_full_frame(&self.frame_queue_tx, ch);
            } else {
                scan_area.live_image.clear_texture(encoder);
            }
            if let Some(unit) = state.unit() {
                scan_area.live_image.unit = unit;
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
            let state = self.channel_state.peek();
            if let Some(ch) = state.selection {
                Self::request_full_frame(&self.frame_queue_tx, ch);
            } else {
                scan_area.live_image.clear_texture(encoder);
            }
            if let Some(unit) = state.unit() {
                scan_area.live_image.unit = unit;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScanStatus {
    pub scan_dir: ScanDir,
    pub line_dir: LineDir,
    pub line_number: u32,
    pub scanning: bool,
}
impl Default for ScanStatus {
    fn default() -> Self {
        Self {
            scan_dir: ScanDir::Down,
            line_number: Default::default(),
            line_dir: LineDir::Forward,
            scanning: false,
        }
    }
}
impl ScanStatus {
    pub fn scan_line_position(&self, scan_size: [u32; 2], line_dir: LineDir) -> Option<f64> {
        if !self.scanning {
            return None;
        }
        let mut line_number = self.line_number;
        if line_dir == LineDir::Backward && self.line_dir == LineDir::Forward {
            line_number = line_number.saturating_sub(1)
        }
        let num_rows = scan_size[1];
        if line_number == num_rows {
            return None;
        }
        let mut pos = ((line_number as f64 - 0.5) / num_rows as f64) - 0.5;
        if self.scan_dir == ScanDir::Up {
            pos *= -1.;
        }
        Some(pos)
    }
}

trait Worker: Sized + Send + 'static {
    fn name(&self) -> String;
    fn init(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()>;
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()>;
    fn run(mut self, addr: impl AsRef<str>, port: u16) -> JoinHandle<()> {
        let addr = addr.as_ref().to_string();
        std::thread::Builder::new()
            .name(self.name())
            .spawn(move || self.run_inner(addr, port))
            .unwrap()
    }
    #[instrument(name = "worker", skip(self), fields(name = self.name()))]
    fn run_inner(&mut self, addr: String, port: u16) {
        'reconnect: loop {
            info!("connecting");
            let mut conn = loop {
                if let Ok(conn) = blocking::NanonisTcp::new((addr.as_str(), port)) {
                    break conn;
                }
            };
            info!("connected");
            'retry: loop {
                match self
                    .init(&mut conn)
                    .inspect_err(|e| error!("failed initializing: {}", e))
                {
                    Ok(_) => break,
                    Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Codec(_)) => {
                        std::thread::sleep(Duration::from_secs(1));
                        continue 'retry;
                    }
                    Err(NanonisTcpError::Io(_)) => {
                        std::thread::sleep(Duration::from_secs(1));
                        continue 'reconnect;
                    }
                }
            }
            info!("initialized");
            let mut num_retries = 0;
            'retry: loop {
                match self
                    .work(&mut conn)
                    .inspect_err(|e| error!("failed working: {}", e))
                {
                    Ok(_) => {
                        num_retries = 0;
                    }
                    Err(NanonisTcpError::Api(_)) | Err(NanonisTcpError::Codec(_)) => {
                        let dur = (2f32.powi(num_retries) * 1e-3).max(1.0);
                        let dur = Duration::from_secs_f32(dur);
                        info!("reconnecting after {dur:?}");
                        std::thread::sleep(dur);
                        num_retries += 1;
                        continue 'retry;
                    }
                    Err(NanonisTcpError::Io(_)) => {
                        let dur = Duration::from_secs(1);
                        info!("reconnecting after {dur:?}");
                        std::thread::sleep(dur);
                        continue 'reconnect;
                    }
                }
            }
        }
    }
}

pub fn overwrite_queue<T>(cap: usize) -> (OverwriteQueueSender<T>, OverwriteQueueReceiver<T>) {
    let queue = Arc::new(ArrayQueue::new(cap));
    let parker = Parker::new();
    let unparker = parker.unparker().clone();
    (
        OverwriteQueueSender {
            queue: Arc::clone(&queue),
            unparker,
        },
        OverwriteQueueReceiver { queue, parker },
    )
}

#[derive(Clone)]
struct OverwriteQueueSender<T> {
    queue: Arc<ArrayQueue<T>>,
    unparker: Unparker,
}
impl<T> OverwriteQueueSender<T> {
    pub fn send(&self, value: T) -> Option<T> {
        let overwrote = self.queue.force_push(value);
        self.unparker.unpark();
        overwrote
    }
}

struct OverwriteQueueReceiver<T> {
    queue: Arc<ArrayQueue<T>>,
    parker: Parker,
}
impl<T> OverwriteQueueReceiver<T> {
    pub fn recv(&self) -> T {
        loop {
            if let Some(val) = self.queue.pop() {
                return val;
            }
            self.parker.park();
        }
    }
}
