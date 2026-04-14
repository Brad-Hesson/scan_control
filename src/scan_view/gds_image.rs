use core::f64;
use std::{collections::BTreeMap, path::Path};

use eframe::egui_wgpu;
use egui::{Color32, Id, Ui};
use gdsr::{Cell, Element, Library};
use glam::{DAffine2, Vec2};
use image_compute::gds_image::GDSImageBuffers;
use itertools::Itertools;

use crate::{
    scan_view::{callbacks::GDSImageCallback, view::ScanViewCtx, ImageEncoder},
    utils::vec_interop::Projection,
};

pub struct GDSImage {
    pub transform: DAffine2,
    pub scale: f64,
    buffers: BTreeMap<u16, GDSImageBuffers>,
    colors: BTreeMap<u16, Color32>,
}

impl GDSImage {
    pub fn new(encoder: &ImageEncoder, path: impl AsRef<Path>, transform: DAffine2) -> Self {
        let gds = gdsr::Library::read_file(path, None).unwrap();
        let mut polys = BTreeMap::new();
        for cell in gds.cells().values() {
            draw_cell(&mut polys, &gds, DAffine2::IDENTITY, cell);
        }
        let (min, max) = polys.values().flatten().flatten().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(min, max), v| {
                (
                    Vec2 {
                        x: v.x.min(min.x),
                        y: v.y.min(min.y),
                    },
                    Vec2 {
                        x: v.x.max(max.x),
                        y: v.y.max(max.y),
                    },
                )
            },
        );
        let center = (max + min) / 2.;
        for vert in polys.values_mut().flatten().flatten(){
            *vert -= center;
        }
        let scale = min.distance(max) as f64 / 2f64.sqrt();
        let colors = polys
            .keys()
            .map(|layer| {
                (
                    *layer,
                    *COLORS.into_iter().cycle().nth(*layer as usize).unwrap(),
                )
            })
            .collect();
        let buffers = polys
            .into_iter()
            .map(|(layer, polys)| {
                (
                    layer,
                    GDSImageBuffers::new(&encoder.wgpu_state.device, polys),
                )
            })
            .collect();
        Self {
            transform,
            buffers,
            colors,
            scale,
        }
    }
    pub fn show(&self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        for (layer, bufs) in &self.buffers {
            let callback = egui_wgpu::Callback::new_paint_callback(
                ctx.rect,
                GDSImageCallback {
                    transform: self.transform.into(),
                    image_buffers: bufs.clone(),
                    color: *self.colors.get(layer).unwrap(),
                },
            );
            ui.painter().add(callback);
        }
    }
}

fn draw_cell(
    polys: &mut BTreeMap<u16, Vec<Vec<glam::Vec2>>>,
    lib: &Library,
    transform: DAffine2,
    cell: &Cell,
) {
    for elem in cell.iter_elements() {
        draw_element(polys, lib, transform, elem);
    }
}

fn draw_element(
    polys: &mut BTreeMap<u16, Vec<Vec<glam::Vec2>>>,
    lib: &Library,
    transform: DAffine2,
    elem: &Element,
) {
    #[allow(unused_variables)]
    match elem {
        gdsr::Element::Path(path) => todo!(),
        gdsr::Element::Polygon(polygon) => {
            let points = polygon
                .points()
                .iter()
                .dropping_back(1)
                .map(|p| transform.project_pos2(p.to_world()).as_vec2())
                .collect_vec();
            let layer = polygon.layer().value();
            polys.entry(layer).or_default().push(points);
        }
        gdsr::Element::Box(gds_box) => todo!(),
        gdsr::Element::Node(node) => todo!(),
        gdsr::Element::Text(text) => todo!(),
        gdsr::Element::Reference(reference) => {
            let origin = reference.grid().origin().to_world();
            let transform = transform
                * DAffine2::from_angle_translation(
                    reference.grid().angle() / 180. * 3.14159,
                    origin,
                );
            match reference.instance() {
                gdsr::Instance::Cell(cell_name) => {
                    let cell = lib.get_cell(cell_name).unwrap();
                    draw_cell(polys, lib, transform, cell);
                }
                gdsr::Instance::Element(elem) => {
                    draw_element(polys, lib, transform, elem);
                }
            }
        }
    }
}

trait GDSPoint: Copy {
    fn to_world(self) -> glam::DVec2;
}
impl GDSPoint for gdsr::Point {
    fn to_world(self) -> glam::DVec2 {
        glam::DVec2 {
            x: self.x().to_world(),
            y: self.y().to_world(),
        }
    }
}

trait GDSUnit: Copy {
    fn to_world(self) -> f64;
}
impl GDSUnit for gdsr::Unit {
    fn to_world(self) -> f64 {
        match self {
            gdsr::Unit::Integer(gdsr::IntegerUnit { value, units }) => {
                let scale = units * 1e9;
                value as f64 * scale
            }
            #[allow(unused_variables)]
            gdsr::Unit::Float(gdsr::FloatUnit { value, units }) => todo!(),
        }
    }
}

const COLORS: &[Color32] = &[
    Color32::from_rgb(0xff, 0x9d, 0x9d),
    Color32::from_rgb(0xff, 0x80, 0xa8),
    Color32::from_rgb(0xc0, 0x80, 0xff),
    Color32::from_rgb(0x95, 0x80, 0xff),
    Color32::from_rgb(0x80, 0x86, 0xff),
    Color32::from_rgb(0x80, 0xa8, 0xff),
];
