use core::f64;

use egui::{CollapsingHeader, Color32, DragValue, Frame, Id, Shadow, Stroke, Ui};
use glam::{DAffine2, DMat2, DVec2, IVec2};
use itertools::Itertools as _;

use crate::{
    components::{selectable_list::SelectableList, si_drag::si_drag_value},
    connection::{shared_state::SharedState, ScanArea},
    scan_view::{world_delta_transform, BorderRectangle, ScanViewCtx},
    utils::vec_interop::IntoEgui as _,
    view_object::Object,
};

pub struct CourseMotionState {
    menu_active: bool,
    move_target: DVec2,
    calib_matrix: DMat2,
    voltages: SharedState<DVec2>,
}
impl CourseMotionState {
    pub fn new(voltages: &SharedState<DVec2>) -> Self {
        Self {
            menu_active: false,
            move_target: DVec2::ZERO,
            calib_matrix: DMat2::IDENTITY * 1e-6,
            voltages: voltages.clone(),
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui, object_list: &mut SelectableList<Object>) {
        if self.menu_active {
            object_list.clear_selected();
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
                ui.separator();
                CollapsingHeader::new("Config")
                    .default_open(false)
                    .show_unindented(ui, |ui| {
                        ui.label("Course Motor Matrix:");
                        ui.horizontal(|ui| {
                            ui.add(si_drag_value(&mut self.calib_matrix.x_axis.x));
                            ui.add(si_drag_value(&mut self.calib_matrix.y_axis.x));
                        });
                        ui.horizontal(|ui| {
                            ui.add(si_drag_value(&mut self.calib_matrix.x_axis.y));
                            ui.add(si_drag_value(&mut self.calib_matrix.y_axis.y));
                        });

                        ui.label("Course Motor Amplitudes:");
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
                .transform_point2(self.calib_matrix * p)
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

        let real_course_move = self.calib_matrix * steps.as_dvec2();
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
        self.calib_matrix * DMat2::from_diagonal(voltages)
        // match (voltages.x.is_nan(), voltages.y.is_nan()) {
        //     (false, false) => self.calib_matrix * DMat2::from_diagonal(voltages),
        //     (true, false) => todo!(),
        //     (false, true) => todo!(),
        //     (true, true) => todo!(),
        // }
    }
    fn get_steps(&self) -> IVec2 {
        (pseudo_inverse_mat2(self.steps2real_world()) * self.move_target)
            .round()
            .as_ivec2()
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

fn pseudo_inverse_mat2(m: DMat2) -> DMat2 {
    let eps = f64::EPSILON * 10.;
    let [a, c, b, d] = m.to_cols_array(); // glam is column-major

    let det = a * d - b * c;
    if det.abs() > eps {
        return DMat2::from_cols_array(&[d / det, -c / det, -b / det, a / det]);
    }

    let frob2 = a * a + b * b + c * c + d * d;
    if frob2 <= eps * eps {
        return DMat2::ZERO;
    }

    // rank-1 case
    DMat2::from_cols_array(&[a / frob2, b / frob2, c / frob2, d / frob2])
}
