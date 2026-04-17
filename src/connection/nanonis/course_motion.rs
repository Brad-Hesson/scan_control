use core::f64;

use egui::{
    epaint::PathStroke, Button, CollapsingHeader, Color32, DragValue, Frame, Id, Shadow, Shape,
    Stroke, Ui, Vec2,
};
use glam::{DAffine2, DMat2, DVec2, IVec2};
use itertools::Itertools as _;
use tracing::info;

use crate::{
    components::{selectable_list::SelectableList, si_drag::si_drag_value},
    connection::{
        nanonis::command_channel::CommandChannelSender, shared_state::SharedState, ScanArea,
    },
    scan_view::{world_delta_transform, BorderRectangle, ScanViewCtx},
    utils::vec_interop::IntoEgui as _,
    view_object::Object,
};

pub struct CourseMotionState {
    menu_active: bool,
    move_target: DVec2,
    calib_matrix: DMat2,
    voltages: SharedState<DVec2>,
    move_sender: CommandChannelSender<(IVec2, u32), ()>,
    currently_moving: bool,
    group: u32,
    last_x_move: Option<(DVec2, IVec2)>,
    last_y_move: Option<(DVec2, IVec2)>,
    show_history: bool,
}
impl CourseMotionState {
    pub fn new(
        voltages: &SharedState<DVec2>,
        move_sender: &CommandChannelSender<(IVec2, u32), ()>,
    ) -> Self {
        Self {
            menu_active: false,
            move_target: DVec2::ZERO,
            calib_matrix: DMat2::IDENTITY * 1e-6,
            voltages: voltages.clone(),
            move_sender: move_sender.clone(),
            currently_moving: false,
            group: 1,
            last_x_move: None,
            last_y_move: None,
            show_history: false,
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui, object_list: &mut SelectableList<Object>) {
        if self.menu_active {
            object_list.clear_selected();
        }
        if self.move_sender.poll_complete().is_some() {
            self.currently_moving = false;
            if let Some(scan_area) = object_list
                .iter_mut()
                .find_map(|entry| entry.as_scan_area_mut())
            {
                let steps = self.get_steps();
                let real_course_move = self.calib_matrix * steps.as_dvec2() * 1e9;
                if steps.y == 0 {
                    let last_pos = scan_area.world_transform.translation;
                    self.last_x_move = Some((last_pos, steps));
                } else {
                    self.last_x_move = None;
                }
                if steps.x == 0 {
                    let last_pos = scan_area.world_transform.translation;
                    self.last_y_move = Some((last_pos, steps));
                } else {
                    self.last_y_move = None;
                }
                let real_world_move = self.steps2real_world() * steps.as_dvec2();
                scan_area.course_move_history.push(real_world_move);
                scan_area.world_transform =
                    scan_area.world_transform * DAffine2::from_translation(real_course_move);
            };
            self.move_target = DVec2::ZERO;
        }
        if ui
            .add_enabled(!self.menu_active, egui::Button::new("Course Motion Menu"))
            .clicked()
        {
            self.menu_active = true;
            self.move_target = DVec2::ZERO;
        }
        ui.checkbox(&mut self.show_history, "Show history");
        let mut menu_active = self.menu_active;
        egui::Window::new("Course Motion")
            .frame(
                Frame::window(&ui.ctx().style())
                    .multiply_with_opacity(0.5)
                    .shadow(Shadow::NONE),
            )
            .auto_sized()
            .collapsible(false)
            .enabled(!self.currently_moving)
            .open(&mut menu_active)
            .show(&ui.ctx(), |ui| {
                let mut voltages = *self.voltages.peek();
                let mut steps = self.get_steps();
                ui.horizontal(|ui| {
                    ui.label("X steps: ");
                    ui.add_enabled(voltages.x > 0., DragValue::new(&mut steps.x));
                    ui.label("Y steps: ");
                    ui.add_enabled(voltages.y > 0., DragValue::new(&mut steps.y));
                });
                ui.shrink_width_to_current();
                self.write_steps(steps);
                ui.add_space(12.);
                let mut button_rect = ui.cursor();
                button_rect.set_height(32.);
                button_rect = button_rect.scale_from_center2(Vec2::new(0.5, 1.));
                ui.horizontal(|ui| {
                    let steps = self.get_steps();
                    if ui
                        .add_enabled_ui(steps.x.abs() > 0 || steps.y.abs() > 0, |ui| {
                            ui.put(button_rect, Button::new("Execute Move"))
                        })
                        .inner
                        .clicked()
                    {
                        info!("Executing course move {steps:?} on group {}", self.group);
                        self.currently_moving = true;
                        self.move_sender.send((steps, self.group - 1));
                    }
                    if self.currently_moving {
                        ui.spinner();
                    }
                });
                ui.separator();
                CollapsingHeader::new("Config")
                    .default_open(false)
                    .show_unindented(ui, |ui| {
                        ui.label("Group:");
                        ui.add(DragValue::new(&mut self.group).speed(0).range(1..=4));
                        ui.label("Driver Amplitudes:");
                        ui.horizontal(|ui| {
                            ui.add_enabled(false, DragValue::new(&mut voltages.x));
                            ui.add_enabled(false, DragValue::new(&mut voltages.y));
                        });
                        ui.label("Calibration Matrix:");
                        egui::Grid::new("course cal grid").show(ui, |ui| {
                            ui.add(si_drag_value(&mut self.calib_matrix.x_axis.x));
                            ui.add(si_drag_value(&mut self.calib_matrix.y_axis.x));
                            ui.end_row();
                            ui.add(si_drag_value(&mut self.calib_matrix.x_axis.y));
                            ui.add(si_drag_value(&mut self.calib_matrix.y_axis.y));
                        });
                        ui.label("Auto Calibration:");
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled_ui(self.last_x_move.is_some(), |ui| {
                                    ui.button("Calibrate X")
                                })
                                .inner
                                .clicked()
                            {
                                if let Some(scan_area) = object_list
                                    .iter_mut()
                                    .find_map(|entry| entry.as_scan_area_mut())
                                {
                                    let (last_pos, steps) = self.last_x_move.take().unwrap();
                                    let world_motion =
                                        scan_area.world_transform.translation - last_pos;
                                    let real_world_motion = scan_area
                                        .world_transform
                                        .inverse()
                                        .transform_vector2(world_motion);
                                    self.calib_matrix.x_axis =
                                        real_world_motion / (steps.x as f64 * voltages.x) * 1e-9;
                                }
                            }
                            if ui
                                .add_enabled_ui(self.last_y_move.is_some(), |ui| {
                                    ui.button("Calibrate Y")
                                })
                                .inner
                                .clicked()
                            {
                                if let Some(scan_area) = object_list
                                    .iter_mut()
                                    .find_map(|entry| entry.as_scan_area_mut())
                                {
                                    let (last_pos, steps) = self.last_y_move.take().unwrap();
                                    let world_motion =
                                        scan_area.world_transform.translation - last_pos;
                                    let real_world_motion = scan_area
                                        .world_transform
                                        .inverse()
                                        .transform_vector2(world_motion);
                                    self.calib_matrix.y_axis =
                                        real_world_motion / (steps.y as f64 * voltages.y) * 1e-9;
                                }
                            }
                        });
                    });
            });
        self.menu_active = menu_active;
    }
    pub fn show_overlay(&mut self, ui: &mut Ui, object_list: &mut SelectableList<Object>) {
        let Some(scan_area) = object_list
            .iter_mut()
            .find_map(|entry| entry.as_scan_area_mut())
        else {
            return;
        };
        if self.show_history {
            self.show_history(ui, scan_area);
        }
        if self.menu_active {
            if ui.input(|i| i.modifiers.ctrl) {
                let [_, _, translate] = world_delta_transform(ui);
                let world_translate = translate.translation;
                let scan_world_translate = scan_area
                    .world_transform
                    .inverse()
                    .transform_vector2(world_translate);
                self.move_target += scan_world_translate;
            }
            self.show_current_move(ui, scan_area);
        }
    }
    fn show_history(&self, ui: &mut Ui, scan_area: &ScanArea) {
        let mut rel_pos = DVec2::ZERO;
        for real_world_move in scan_area.course_move_history.iter().rev().copied() {
            rel_pos -= real_world_move;
            let real_world_transform =
                DAffine2::from_scale_angle_translation(scan_area.area_size, 0., rel_pos);
            BorderRectangle {
                transform: scan_area.world_transform * real_world_transform,
                color: Color32::YELLOW,
                dashed: false,
            }
            .show(ui);
        }
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let real_to_screen = ctx.world2egui() * scan_area.world_transform;
        let mut rel_pos = DVec2::ZERO;
        let mut last_rel_pos = DVec2::ZERO;
        for real_world_move in scan_area.course_move_history.iter().rev().copied() {
            rel_pos -= real_world_move;
            let p0 = real_to_screen.transform_point2(rel_pos);
            let p1 = real_to_screen.transform_point2(last_rel_pos);
            let v_move = p0 - p1;
            let v_side = v_move.normalize() * f64::min(15., v_move.length() / 2.);
            let p2 = DMat2::from_angle(0.5) * v_side + p1;
            let p3 = DMat2::from_angle(-0.5) * v_side + p1;
            let p0 = p0.to_egui_pos2();
            let p1 = p1.to_egui_pos2();
            let p2 = p2.to_egui_pos2();
            let p3 = p3.to_egui_pos2();
            ui.painter()
                .line_segment([p0, p1], Stroke::new(2., Color32::ORANGE));
            ui.painter().add(Shape::convex_polygon(
                vec![p1, p2, p3],
                Color32::ORANGE,
                Stroke::NONE,
            ));
            last_rel_pos = rel_pos;
        }
    }
    fn show_current_move(&self, ui: &mut Ui, scan_area: &ScanArea) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();

        let world2screen = ctx.world2egui();
        let course_world2screen = |p: DVec2| {
            (world2screen * scan_area.world_transform)
                .transform_point2(self.calib_matrix * p * 1e9)
                .to_egui_pos2()
        };
        let steps = self.get_steps();

        std::iter::once(DVec2::ZERO)
            .chain(course_path_iter(steps))
            .map(course_world2screen)
            .tuple_windows()
            .for_each(|(a, b)| {
                ui.painter()
                    .line_segment([a, b], Stroke::new(1., Color32::ORANGE));
            });
        course_path_iter(steps)
            .map(course_world2screen)
            .for_each(|point| {
                ui.painter().circle_filled(point, 4., Color32::ORANGE);
            });

        let real_course_move = self.steps2real_world() * steps.as_dvec2();
        let move_transform =
            DAffine2::from_scale_angle_translation(scan_area.area_size, 0., real_course_move);
        BorderRectangle {
            transform: scan_area.world_transform * move_transform,
            color: Color32::YELLOW,
            dashed: false,
        }
        .show(ui);
    }
    fn steps2real_world(&self) -> DMat2 {
        let voltages = *self.voltages.peek();
        self.calib_matrix * DMat2::from_diagonal(voltages) * 1e9
    }
    fn get_steps(&self) -> IVec2 {
        let m = self.steps2real_world();
        let mat = nalgebra::Matrix2::<f64>::from(m);
        let inv = DMat2::from(mat.pseudo_inverse(f64::EPSILON * 10.).unwrap());
        (inv * self.move_target).round().as_ivec2()
    }
    fn write_steps(&mut self, steps: IVec2) {
        let change = steps - self.get_steps();
        self.move_target += self.steps2real_world() * change.as_dvec2();
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
