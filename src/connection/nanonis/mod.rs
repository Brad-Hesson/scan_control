mod channel_state;
mod scan_status;
mod worker;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub use channel_state::ChannelState;
use egui::{Color32, DragValue, Frame, Id, Shadow, Stroke, Ui};
use glam::{DAffine2, DMat2, DVec2, IVec2, Mat2};
use itertools::Itertools;
use nanonis_tcp::LineDir;
pub use scan_status::ScanStatus;
use tracing::trace;
use uuid::Uuid;

use crate::{
    components::selectable_list::{SelectableEntry, SelectableList},
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
    scan_view::{world_delta_transform, BorderRectangle, ImageEncoder, ScanViewCtx},
    utils::vec_interop::IntoEgui,
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
    course_amp: SharedState<[f32; 2]>,
    slow_status_init: Arc<AtomicBool>,
    fast_status_init: Arc<AtomicBool>,
    course_menu_active: bool,
    course_move_target: DVec2,
    course_matrix: DMat2,
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
        if self.course_menu_active {
            object_list.clear_selected();
        }
        let Some(scan_area) = object_list
            .iter_mut()
            .find_map(|entry| entry.as_scan_area_mut())
        else {
            return;
        };
        scan_area.show_menu(ui, encoder);
        if ui
            .add_enabled(
                !self.course_menu_active,
                egui::Button::new("Course Motion Menu"),
            )
            .clicked()
        {
            self.course_move_target = DVec2::ZERO;
            self.course_menu_active = true;
        }
        egui::Window::new("Course Motion")
            .frame(
                Frame::window(&ui.ctx().style())
                    .multiply_with_opacity(0.5)
                    .shadow(Shadow::NONE),
            )
            .default_size([200., 400.])
            .collapsible(false)
            .resizable(true)
            .scroll([false, true])
            .open(&mut self.course_menu_active)
            .show(&ui.ctx(), |ui| {
                let course_vec = (self.course_matrix.inverse() * self.course_move_target).round();
                let mut course_x = course_vec.x as i32;
                let mut course_y = course_vec.y as i32;
                ui.heading("Steps:");
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut course_x));
                    ui.add(DragValue::new(&mut course_y));
                });
                let change = DVec2::new(
                    course_x as f64 - course_vec.x,
                    course_y as f64 - course_vec.y,
                );
                self.course_move_target += self.course_matrix * change;
                ui.heading("Course Motor Matrix:");
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut self.course_matrix.x_axis.x));
                    ui.add(DragValue::new(&mut self.course_matrix.y_axis.x));
                });
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut self.course_matrix.x_axis.y));
                    ui.add(DragValue::new(&mut self.course_matrix.y_axis.y));
                });
            });
    }
    fn show_image_view_overlay(
        &mut self,
        ui: &mut Ui,
        object_list: &mut SelectableList<view_object::Object>,
    ) {
        if !self.course_menu_active {
            return;
        }
        let Some(scan_area) = object_list
            .iter_mut()
            .find_map(|entry| entry.as_scan_area_mut())
        else {
            return;
        };
        if ui.input(|i| i.modifiers.ctrl) {
            let [_, _, translate] = world_delta_transform(ui, DVec2::ZERO);
            let world_translate = translate.translation;
            let scan_world_translate = scan_area
                .world_transform
                .inverse()
                .transform_vector2(world_translate);
            self.course_move_target += scan_world_translate;
        }
        let course_steps = (self.course_matrix.inverse() * self.course_move_target).round();
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let world2screen = ctx.world2egui();
        for (a, b) in std::iter::once(DVec2::ZERO)
            .chain(course_path_iter(course_steps.as_ivec2()))
            .tuple_windows()
        {
            let a = (world2screen * scan_area.world_transform)
                .transform_point2(self.course_matrix * a)
                .to_egui_pos2();
            let b = (world2screen * scan_area.world_transform)
                .transform_point2(self.course_matrix * b)
                .to_egui_pos2();
            ui.painter()
                .line_segment([a, b], Stroke::new(1., Color32::ORANGE));
        }
        for point in course_path_iter(course_steps.as_ivec2()) {
            let center = (world2screen * scan_area.world_transform)
                .transform_point2(self.course_matrix * point)
                .to_egui_pos2();
            ui.painter().circle_filled(center, 4., Color32::ORANGE);
        }

        let real_course_move = self.course_matrix * course_steps;
        let move_transform =
            DAffine2::from_scale_angle_translation(*self.area_size.peek(), 0., real_course_move);
        BorderRectangle {
            transform: scan_area.world_transform * move_transform,
            color: Color32::YELLOW,
            dashed: false,
        }
        .show(ui);
    }
}

fn course_path_iter(steps: IVec2) -> impl Iterator<Item = DVec2> {
    integer_iter(steps.x)
        .map(move |x| IVec2 { x, y: 0 })
        .chain(integer_iter(steps.y).map(move |y| IVec2 { x: steps.x, y }))
        .map(|v| v.as_dvec2())
}
fn integer_iter(end: i32) -> impl Iterator<Item = i32> {
    let del = end.signum();
    (0..end.abs()).map(move |v| (v + 1) * del)
}

#[test]
fn feature() {
    dbg!(course_path_iter(IVec2::new(0, 0)).collect_vec());
    dbg!(course_path_iter(IVec2::new(-3, 4)).collect_vec());
    dbg!(course_path_iter(IVec2::new(2, 5)).collect_vec());
    dbg!(course_path_iter(IVec2::new(3, -2)).collect_vec());
    dbg!(course_path_iter(IVec2::new(-1, -2)).collect_vec());
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
        let course_amp = SharedState::new_default();
        let (frame_queue_tx, frame_queue_rx) = overwrite_queue(2);
        let slow_status_init = Arc::new(AtomicBool::new(false));
        let fast_status_init = Arc::new(AtomicBool::new(false));

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
            &course_amp,
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
            course_amp,
            slow_status_init,
            fast_status_init,
            base_name,
            course_menu_active: false,
            course_move_target: DVec2::ZERO,
            course_matrix: DMat2::IDENTITY * 1e3,
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
