use egui::{
    epaint::{ColorMode, PathShape, PathStroke},
    Color32, Id, Pos2, Shape, StrokeKind, Ui,
};
use glam::{DAffine2, DVec2};
use itertools::Itertools;

use crate::{scan_view::view::ScanViewCtx, utils::vec_interop::IntoGlam as _};

pub struct BorderRectangle {
    pub transform: DAffine2,
    pub color: Color32,
    pub dashed: bool,
}
impl BorderRectangle {
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let t = DAffine2::from_translation(ctx.rect.center().to_glam())
            * ctx.world_transform
            * self.transform;
        let p0: [f32; 2] = t
            .transform_point2(DVec2::new(-0.5, -0.5))
            .to_array()
            .into_iter()
            .map(|v| v as f32)
            .collect_array()
            .unwrap();
        let p1: [f32; 2] = t
            .transform_point2(DVec2::new(0.5, -0.5))
            .to_array()
            .into_iter()
            .map(|v| v as f32)
            .collect_array()
            .unwrap();
        let p2: [f32; 2] = t
            .transform_point2(DVec2::new(0.5, 0.5))
            .to_array()
            .into_iter()
            .map(|v| v as f32)
            .collect_array()
            .unwrap();
        let p3: [f32; 2] = t
            .transform_point2(DVec2::new(-0.5, 0.5))
            .to_array()
            .into_iter()
            .map(|v| v as f32)
            .collect_array()
            .unwrap();
        let mut points = vec![p0.into(), p1.into(), p2.into(), p3.into()];
        let stroke = PathStroke {
            width: 2.,
            color: ColorMode::Solid(self.color),
            kind: StrokeKind::Outside,
        };
        if self.dashed {
            points.push(p0.into());
            ui.painter().add(dashes_from_line(&points, stroke, 6., 3.));
        } else {
            ui.painter().add(PathShape {
                points,
                closed: true,
                fill: Color32::TRANSPARENT,
                stroke,
            });
        }
    }
}

/// Creates dashes from a line.
fn dashes_from_line(
    path: &[Pos2],
    stroke: PathStroke,
    dash_length: f32,
    gap_length: f32,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let mut position_on_segment = 0.;
    let mut drawing_dash = false;
    for window in path.windows(2) {
        let (start, end) = (window[0], window[1]);
        let vector = end - start;
        let segment_length = vector.length();

        let mut start_point = start;
        while position_on_segment < segment_length {
            let new_point = start + vector * (position_on_segment / segment_length);
            if drawing_dash {
                // This is the end point.
                shapes.push(Shape::Path(PathShape {
                    points: [start_point, new_point].into(),
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: stroke.clone(),
                }));
                position_on_segment += gap_length;
            } else {
                // Start a new dash.
                start_point = new_point;
                position_on_segment += dash_length;
            }
            drawing_dash = !drawing_dash;
        }

        // If the segment ends and the dash is not finished, add the segment's end point.
        if drawing_dash {
            shapes.push(Shape::Path(PathShape {
                points: [start_point, end].into(),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: stroke.clone(),
            }));
        }

        position_on_segment -= segment_length;
    }
    shapes
}
