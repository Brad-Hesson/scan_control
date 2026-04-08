mod channel_state;
mod scan_status;
mod worker;

pub use channel_state::ChannelState;
use glam::{DAffine2, DVec2};
use nanonis_tcp::LineDir;
pub use scan_status::ScanStatus;
use tracing::trace;

use crate::{
    connection::{
        live_image::{FrameData, LiveImage},
        nanonis::worker::{
            FastStatusWorker, FrameWorker, LineWorker, SlowStatusWorker, Worker as _,
        },
        queue::{overwrite_queue, OverwriteQueueSender},
        scan_area::ScanArea,
        shared_state::SharedState,
        Connection,
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
    frame_queue_tx: OverwriteQueueSender<LineDir>,
    tip_pos: SharedState<DVec2>,
}

impl Connection for NanonisConnection {
    fn poll_connected(&mut self, encoder: &ImageEncoder) -> Option<ScanArea> {
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

    fn update(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) -> Option<LiveImage> {
        self.update_channel(scan_area, encoder);
        self.update_area_size(scan_area);
        self.update_image_transform(scan_area);
        self.update_image_data(&mut scan_area.live_image, encoder);
        self.update_scan_status(scan_area);
        self.update_tip_pos(scan_area);
        None
    }
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
        let (frame_queue_tx, frame_queue_rx) = overwrite_queue(4);
        let address = address.as_ref();
        LineWorker::new(&frame_queue_tx, &scan_status).run(address, 6501);
        FrameWorker::new(
            &ctx,
            &forward_data,
            &backward_data,
            frame_queue_rx,
            &channel_state,
            &scan_status,
        )
        .run(address, 6502);
        FastStatusWorker::new(&ctx, &image_transform, &tip_pos).run(address, 6503);
        SlowStatusWorker::new(&ctx, &area_size, &channel_state, &scan_status).run(address, 6504);
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
            if live_image.line_dir == LineDir::Forward {
                live_image.write_and_update_texture(encoder);
            }
        }
        if let Some(backward_data) = self.backward_data.read_new() {
            live_image.backward_data = backward_data.clone();
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
    fn request_full_frame(tx: &OverwriteQueueSender<LineDir>) {
        trace!("requesting full frame");
        tx.send(LineDir::Forward);
        tx.send(LineDir::Backward);
    }
    fn update_channel(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) {
        // if nanonis sent a change to the channel
        if let Some(state) = self.channel_state.read_new() {
            scan_area.channel_opts = state.channel_opts_names().collect();
            if let Some(unit) = state.unit() {
                scan_area.live_image.unit = unit;
            }
            let new_selected_string = state.selected_as_string();
            if scan_area.channel_selected != new_selected_string {
                scan_area.channel_selected = new_selected_string;
                if state.selection.is_some() {
                    Self::request_full_frame(&self.frame_queue_tx);
                } else {
                    scan_area.live_image.clear_texture(encoder);
                }
            }
        }
        // if we changed the selected channel
        if self.channel_state.modify_conditional(
            |prev| prev.selected_as_string() != scan_area.channel_selected,
            |state| {
                match &scan_area.channel_selected {
                    Some(ch_name) => state.set_selection_by_name(&ch_name),
                    None => state.selection = None,
                };
                if let Some(unit) = state.unit() {
                    scan_area.live_image.unit = unit;
                }
            },
        ) {
            if scan_area.channel_selected.is_some() {
                Self::request_full_frame(&self.frame_queue_tx);
            } else {
                scan_area.live_image.clear_texture(encoder);
            }
        }
    }
}
