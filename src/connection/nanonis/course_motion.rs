use core::f64;

use egui::{Button, CollapsingHeader, Color32, DragValue, Frame, Id, Shadow, Stroke, Ui};
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
    last_move: Option<(DVec2, IVec2)>,
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
            last_move: None,
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui, object_list: &mut SelectableList<Object>) {
        if self.menu_active {
            object_list.clear_selected();
        }
        if self.move_sender.poll_complete().is_some() {
            self.currently_moving = false;
            self.menu_active = false;
            if let Some(scan_area) = object_list
                .iter_mut()
                .find_map(|entry| entry.as_scan_area_mut())
            {
                let steps = self.get_steps();
                let real_course_move = self.calib_matrix * steps.as_dvec2() * 1e9;
                let last_pos = scan_area.world_transform.translation;
                self.last_move = Some((last_pos, steps));
                scan_area.world_transform =
                    scan_area.world_transform * DAffine2::from_translation(real_course_move);
            };
        }
        if ui
            .add_enabled(!self.menu_active, egui::Button::new("Course Motion Menu"))
            .clicked()
        {
            self.move_target = DVec2::ZERO;
            self.menu_active = true;
        }
        let mut menu_active = self.menu_active;
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
            .enabled(!self.currently_moving)
            .open(&mut menu_active)
            .show(&ui.ctx(), |ui| {
                let mut voltages = *self.voltages.peek();
                ui.label("Steps:");
                let mut steps = self.get_steps();
                ui.horizontal(|ui| {
                    ui.add_enabled(voltages.x > 0., DragValue::new(&mut steps.x));
                    ui.add_enabled(voltages.y > 0., DragValue::new(&mut steps.y));
                });
                self.write_steps(steps);
                if ui
                    .add_enabled(
                        voltages.x > 0. || voltages.y > 0.,
                        Button::new("Execute Move"),
                    )
                    .clicked()
                {
                    let steps = self.get_steps();
                    info!("Executing course move {steps:?} on group {}", self.group);
                    self.currently_moving = true;
                    self.move_sender.send((steps, self.group - 1));
                }
                ui.separator();
                CollapsingHeader::new("Config")
                    .default_open(false)
                    .show_unindented(ui, |ui| {
                        ui.label("Group:");
                        ui.add(DragValue::new(&mut self.group).speed(0).range(1..=4));
                        ui.label("Calibration Matrix:");
                        ui.horizontal(|ui| {
                            ui.add(si_drag_value(&mut self.calib_matrix.x_axis.x));
                            ui.add(si_drag_value(&mut self.calib_matrix.y_axis.x));
                        });
                        ui.horizontal(|ui| {
                            ui.add(si_drag_value(&mut self.calib_matrix.x_axis.y));
                            ui.add(si_drag_value(&mut self.calib_matrix.y_axis.y));
                        });

                        ui.label("Driver Amplitudes:");
                        ui.horizontal(|ui| {
                            ui.add_enabled(false, DragValue::new(&mut voltages.x));
                            ui.add_enabled(false, DragValue::new(&mut voltages.y));
                        });
                    });
            });
        self.menu_active = menu_active;
    }
    pub fn show_overlay(&mut self, ui: &mut Ui, object_list: &mut SelectableList<Object>) {
        if !self.menu_active {
            return;
        }
        let Some(scan_area) = object_list
            .iter_mut()
            .find_map(|entry| entry.as_scan_area_mut())
        else {
            return;
        };
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

        let real_course_move = self.calib_matrix * steps.as_dvec2() * 1e9;
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
