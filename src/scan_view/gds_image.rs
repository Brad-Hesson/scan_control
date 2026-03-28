use std::path::Path;

use egui::{epaint::PathShape, Color32, Id, Shape, Stroke, Ui};
use itertools::Itertools;

use crate::scan_view::{file_image::ProjectionTransform, view::ScanViewCtx};

pub struct GDSImage {
    library: gdsr::Library,
}

impl GDSImage {
    pub fn new(p: impl AsRef<Path>) -> Self {
        let gds = gdsr::Library::read_file(p, Some(1e-9)).unwrap();
        Self { library: gds }
    }
    pub fn show(&self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        for (name, cell) in self.library.cells() {
            let elements = cell.get_elements(None, &self.library);
            for elem in elements {
                match elem {
                    gdsr::Element::Path(path) => todo!(),
                    gdsr::Element::Polygon(polygon) => {
                        let points = polygon
                            .points()
                            .iter()
                            .map(|p| ctx.world2egui().project_egui_pos(p.to_world()))
                            .collect_vec();
                        let shape =
                            Shape::Path(PathShape::line(points, Stroke::new(2., Color32::PURPLE)));

                        ui.painter().add(shape);
                    }
                    gdsr::Element::Box(gds_box) => todo!(),
                    gdsr::Element::Node(node) => todo!(),
                    gdsr::Element::Text(text) => todo!(),
                    gdsr::Element::Reference(reference) => todo!(),
                }
            }
        }
    }
}

trait GDSPoint: Copy {
    fn to_world(self) -> egui::Pos2;
}
impl GDSPoint for gdsr::Point {
    fn to_world(self) -> egui::Pos2 {
        egui::Pos2 {
            x: self.x().float_value() as f32,
            y: self.y().float_value() as f32,
        }
    }
}
