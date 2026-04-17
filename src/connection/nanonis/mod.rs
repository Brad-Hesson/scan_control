mod channel_state;
mod command_channel;
mod course_motion;
mod scan_status;
mod worker;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub use channel_state::ChannelState;
use egui::Ui;
use glam::{DAffine2, DVec2};
use itertools::Itertools;
use nanonis_tcp::LineDir;
pub use scan_status::ScanStatus;
use tracing::trace;
use uuid::Uuid;

use crate::{
    components::selectable_list::{SelectableEntry, SelectableList},
    connection::{
        live_image::{FrameData, LiveImage},
        nanonis::{
            course_motion::CourseMotionState,
            worker::{FastStatusWorker, FrameWorker, LineWorker, SlowStatusWorker, Worker as _},
        },
        queue::{overwrite_queue, OverwriteQueueSender},
        scan_area::ScanArea,
        shared_state::SharedState,
        Connection,
    },
    scan_view::ImageEncoder,
    view_object::{self, Object},
};

pub struct NanonisConnection {
    forward_data: SharedState<FrameData>,
    backward_data: SharedState<FrameData>,
    image_transform: SharedState<DAffine2>,
    area_size: SharedState<DVec2>,
    channel_state: SharedState<ChannelState>,
    scan_status: SharedState<ScanStatus>,
    base_name: SharedState<String>,
    frame_queue_tx: OverwriteQueueSender<LineDir>,
    tip_pos: SharedState<DVec2>,
    slow_status_init: Arc<AtomicBool>,
    fast_status_init: Arc<AtomicBool>,
    course_menu: CourseMotionState,
}

impl Connection for NanonisConnection {
    fn poll_connected(
        &mut self,
        object_list: &mut SelectableList<view_object::Object>,
        encoder: &ImageEncoder,
    ) -> bool {
        if self.slow_status_init.load(Ordering::SeqCst)
            && self.fast_status_init.load(Ordering::SeqCst)
        {
            if object_list.iter().any(|obj| obj.as_scan_area().is_some()) {
                return true;
            }
            let scan_area = ScanArea::new(
                encoder,
                *self.area_size.read(),
                *self.image_transform.read(),
                *self.tip_pos.read(),
            );
            object_list.push(SelectableEntry::new(
                "area",
                Object::ScanArea(scan_area),
                |img| img.list_atoms(),
            ));
            true
        } else {
            false
        }
    }

    fn update(
        &mut self,
        object_list: &mut SelectableList<view_object::Object>,
        encoder: &ImageEncoder,
    ) {
        let Some((index, scan_area)) = object_list
            .iter_mut()
            .enumerate()
            .find_map(|(i, entry)| entry.as_scan_area_mut().map(|area| (i, area)))
        else {
            return;
        };
        self.update_channel(scan_area, encoder);
        self.update_area_size(scan_area);
        self.update_image_transform(scan_area);
        self.update_image_data(&mut scan_area.live_image, encoder);
        self.update_scan_status(scan_area);
        self.update_tip_pos(scan_area);
        self.update_base_name(scan_area);
        let stamps = scan_area.stamp.drain(..).collect_vec();
        let base_name = scan_area.stamp_name_base.clone();

        let mut name_index = 0;
        for stamp in stamps {
            'try_again: loop {
                for obj in object_list.iter() {
                    let name = obj.name();
                    let Some(rest) = name.strip_prefix(&base_name) else {
                        continue;
                    };
                    if rest.is_empty() && name_index == 0 {
                        name_index += 1;
                        continue 'try_again;
                    }
                    let Some(existing_name_index) = rest
                        .strip_prefix("(")
                        .and_then(|rest| rest.strip_suffix(")"))
                        .and_then(|num_str| num_str.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    if existing_name_index == name_index {
                        name_index += 1;
                        continue 'try_again;
                    }
                }
                break;
            }
            let name = if name_index == 0 {
                base_name.clone()
            } else {
                format!("{}({})", base_name, name_index)
            };
            name_index += 1;
            object_list.insert(
                index,
                SelectableEntry::new(
                    Uuid::new_v4(),
                    Object::ScanImage { image: stamp, name },
                    |img| img.list_atoms(),
                ),
            );
        }
    }
    fn show_menu(
        &mut self,
        ui: &mut Ui,
        object_list: &mut SelectableList<view_object::Object>,
        encoder: &ImageEncoder,
    ) {
        ui.set_max_width(140.);
        let Some(scan_area) = object_list
            .iter_mut()
            .find_map(|entry| entry.as_scan_area_mut())
        else {
            return;
        };
        scan_area.show_menu(ui, encoder);
        ui.separator();
        self.course_menu.show_menu(ui, object_list);
    }
    fn show_image_view_overlay(
        &mut self,
        ui: &mut Ui,
        object_list: &mut SelectableList<view_object::Object>,
    ) {
        self.course_menu.show_overlay(ui, object_list);
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
        let base_name = SharedState::new_default();
        let tip_pos = SharedState::new_default();
        let course_voltages = SharedState::new_default();
        let (frame_queue_tx, frame_queue_rx) = overwrite_queue(2);
        let slow_status_init = Arc::new(AtomicBool::new(false));
        let fast_status_init = Arc::new(AtomicBool::new(false));
        let (move_sender, move_receiver) = command_channel::command_channel();

        let course_menu = CourseMotionState::new(&course_voltages, &move_sender);

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
        FastStatusWorker::new(&ctx, &image_transform, &tip_pos, &fast_status_init)
            .run(address, 6503);
        SlowStatusWorker::new(
            &ctx,
            &area_size,
            &channel_state,
            &scan_status,
            &base_name,
            &course_voltages,
            &move_receiver,
            &slow_status_init,
        )
        .run(address, 6504);
        Self {
            image_transform,
            area_size,
            channel_state,
            backward_data,
            forward_data,
            frame_queue_tx,
            scan_status,
            tip_pos,
            slow_status_init,
            fast_status_init,
            base_name,
            course_menu,
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
    fn update_base_name(&mut self, scan_area: &mut ScanArea) {
        if let Some(new_name) = self.base_name.read_new().as_deref().cloned() {
            scan_area.stamp_name_base = new_name;
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
