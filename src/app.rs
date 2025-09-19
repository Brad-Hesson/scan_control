use std::fmt::Display;
use std::path::Path;

use crate::components::file_dialog::ViewportFileDialog;
use crate::components::selectable_list::{SelectableEntry, SelectableList};
use crate::scan_view::{BorderRectangle, FitData, ImageEncoder, ScanImage, ScanView};
use crate::undo_queue::{StateModify, UndoQueue};
use crate::utils::response_group::ResponseGroupExt as _;
use eframe::egui_wgpu::RenderState;
use egui::Color32;
use egui::{Align2, Atoms, Button, Frame, Image, IntoAtoms, MenuBar, Ui};
use egui_file_dialog::FileDialog;
use eyre::{Context, Result};
use glam::{Affine2, Vec2};
use itertools::Itertools;
use sxmfile::SXM;
use tracing::{error, info, warn};

pub const COLOR_MAP_SIZE: usize = 256;

pub struct MyApp {
    file_dialog: ViewportFileDialog,
    app_state: AppState,
    image_encoder: ImageEncoder,
    undo_queue: UndoQueue<AppState>,
}

pub struct AppState {
    scan_view: ScanView,
    image_list: SelectableList<StaticImage>,
    current_scan: ScanImage,
    current_scan_src: Box<[f32]>,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu = cc.wgpu_render_state.as_ref().unwrap();
        // let gradient = Gradient::default();
        // scan_view.set_color_map(
        //     gradient
        //         .linear_eval(ScanView::COLOR_MAP_SIZE, true)
        //         .try_into()
        //         .expect("must be a ScanView::COLOR_MAP_SIZE bug"),
        // );

        let src_sxm = SXM::parse_file("20240229_075.sxm").unwrap();
        let width = 97;
        let mut current_scan_src = vec![];
        let src_size = src_sxm.get_image_size().unwrap();
        for y in 0..src_size[1] as usize {
            let line = &src_sxm.data[0][0][y * src_size[0] as usize..][..width];
            current_scan_src.extend_from_slice(line);
        }
        Self {
            app_state: AppState {
                scan_view: ScanView::new(wgpu),
                image_list: SelectableList::new(),
                current_scan: ScanImage::new(
                    wgpu,
                    [width as u32, 512],
                    0,
                    Affine2::from_scale([width as _, 512.].into()),
                    |_| {},
                ),
                current_scan_src: current_scan_src.into_boxed_slice(),
            },
            image_encoder: ImageEncoder::new(wgpu),
            file_dialog: ViewportFileDialog::new(FileDialog::new().title("Import File")),
            undo_queue: UndoQueue::new(),
        }
    }
    pub fn mod_state<T: StateModify<AppState>>(&mut self, modifier: T) {
        self.undo_queue.push(&mut self.app_state, modifier);
    }
    fn load_file(&mut self, wgpu_state: &RenderState, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        info!("Trying to load image `{}`", path.display());
        let sxm_file = sxmfile::SXM::parse_file(path)?;
        info!("Loaded image `{}`", path.display());
        let static_image = StaticImage::load_sxm(sxm_file, wgpu_state)?;
        static_image
            .image_data
            .write_texture_plane_fit_subtract(wgpu_state, &mut self.image_encoder);
        let entry = SelectableEntry::new(static_image, image_list_item);
        self.mod_state(LoadImageModifier(Some(entry)));
        Ok(())
    }
    // fn update_gradient(&mut self) {
    //     if self.gradient != self.last_gradient {
    //         self.last_gradient = self.gradient.clone();
    //         self.scan_view.set_color_map(
    //             self.gradient
    //                 .linear_eval(ScanView::COLOR_MAP_SIZE, true)
    //                 .try_into()
    //                 .expect("must be a ScanView::COLOR_MAP_SIZE bug"),
    //         );
    //     }
    // }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(paths) = self.file_dialog.take_picked_multiple() {
            for path in paths {
                if let Err(e) = self
                    .load_file(frame.wgpu_render_state().unwrap(), path)
                    .context("file load failed")
                {
                    error!("{e:#}");
                }
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            let is_fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fs));
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F)) {
            let indexes = self
                .app_state
                .image_list
                .iter_selected_indexes()
                .collect_vec();
            self.mod_state(MoveForwardModifier(indexes));
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::B)) {
            let indexes = self
                .app_state
                .image_list
                .iter_selected_indexes()
                .collect_vec();
            self.mod_state(MoveBackwardModifier(indexes));
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
            self.undo_queue.undo(&mut self.app_state);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y)) {
            self.undo_queue.redo(&mut self.app_state);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
            && !self.app_state.current_scan.is_full()
        {
            let render_state = frame.wgpu_render_state().unwrap();
            let [x, y] = self.app_state.current_scan.current_size();
            let line = &self.app_state.current_scan_src[x as usize * y as usize..][..x as usize];
            self.app_state
                .current_scan
                .write_line(render_state, line)
                .unwrap();
            self.app_state
                .current_scan
                .write_texture_plane_fit_subtract(render_state, &mut self.image_encoder);
        }
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            MenuBar::new().ui(ui, |ui| {
                file_menu_button(ui, ctx, self);
            });
        });
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            let tr = self.app_state.scan_view.world_transform.inverse();
            let (scale, _, translation) = tr.to_scale_angle_translation();
            ui.label(format!("scale: {scale}, translation: {translation}"));
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                // egui_colorgradient::gradient_editor(ui, &mut self.gradient);
                // self.update_gradient();
                if self
                    .app_state
                    .scan_view
                    .show(ui, |ctx| {
                        let image_list = &mut self.app_state.image_list;
                        for i in 0..image_list.len() {
                            let resp = image_list[i].image_data.show(ctx);
                            resp.clone().synchronize(&mut image_list[i].resp_group);
                            if resp.clicked() {
                                if ctx.ui.input(|i| i.modifiers.ctrl) {
                                    image_list[i].selected = !image_list[i].selected;
                                } else {
                                    image_list.clear_selected();
                                    image_list[i].selected = true;
                                }
                            }
                        }
                        BorderRectangle {
                            transform: self.app_state.current_scan.transform,
                            color: Color32::RED,
                        }
                        .show(ctx);
                        self.app_state.current_scan.show(ctx);
                        if let Some(image) = image_list.get_hovered(ctx.ui.ctx()) {
                            BorderRectangle {
                                transform: image.image_data.transform,
                                color: Color32::LIGHT_BLUE,
                            }
                            .show(ctx);
                        }
                        for image in image_list.iter_selected() {
                            BorderRectangle {
                                transform: image.image_data.transform,
                                color: Color32::GREEN,
                            }
                            .show(ctx);
                        }
                    })
                    .clicked()
                {
                    self.app_state.image_list.clear_selected();
                };
                egui::Window::new("Images")
                    .frame(Frame::window(&ctx.style()).multiply_with_opacity(0.5))
                    .constrain_to(ui.min_rect())
                    .anchor(Align2::LEFT_TOP, egui::Vec2::new(5., 5.))
                    .resizable(false)
                    .show(ctx, |ui| {
                        let vis = &mut ui.style_mut().visuals.widgets.inactive;
                        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
                        self.app_state.image_list.show(ui);
                    });
                let mut new_top = egui::Window::new("Current Scan")
                    .frame(Frame::window(&ctx.style()).multiply_with_opacity(0.5))
                    .constrain_to(ui.min_rect())
                    .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                    .resizable(false)
                    .show(ctx, |ui| image_menu(ui, &self.app_state.current_scan))
                    .unwrap()
                    .response
                    .rect
                    .bottom();
                for i in self.app_state.image_list.iter_selected_indexes() {
                    let name = match self.app_state.image_list[i].image_src.get_name() {
                        Ok(name) => name,
                        Err(e) => {
                            error!("{e:#}");
                            "unnamed"
                        }
                    };
                    let mut rect = ui.min_rect();
                    rect.set_top(new_top);
                    new_top = egui::Window::new(name)
                        .frame(Frame::window(&ctx.style()).multiply_with_opacity(0.5))
                        .constrain_to(rect)
                        .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                        .resizable(false)
                        .show(ctx, |ui| {
                            image_menu(ui, &self.app_state.image_list[i].image_data)
                        })
                        .unwrap()
                        .response
                        .rect
                        .bottom();
                }
            });
    }
}

fn file_menu_button(ui: &mut Ui, ctx: &egui::Context, app: &mut MyApp) {
    ui.menu_button("File", |ui| {
        if ui.add(Button::new("Import")).clicked() {
            app.file_dialog.pick_multiple();
        }
    });
    app.file_dialog.update(ctx);
}
fn image_menu(ui: &mut Ui, image: &ScanImage) {
    let vis = &mut ui.style_mut().visuals.widgets.inactive;
    vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
    if let Some(data) = image.norm_data.read().as_ref() {
        ui.label(format!("Min: {:.2}", MetersFmt(data.min)));
        ui.label(format!("Max: {:.2}", MetersFmt(data.max)));
        ui.label(format!("Std Dev: {:.2}", MetersFmt(data.stddev)));
    }
    if let Some(data) = image.fit_data.read().as_ref() {
        match data {
            FitData::PlaneFit {
                mean,
                x_slope,
                y_slope,
            } => {
                ui.label(format!("Mean: {:.2}", MetersFmt(*mean)));
                ui.label(format!("X Slope: {:.2}", MetersFmt(*x_slope)));
                ui.label(format!("Y Slope: {:.2}", MetersFmt(*y_slope)));
            }
        }
    }
}

struct StaticImage {
    image_data: ScanImage,
    image_src: SXM,
}
impl StaticImage {
    pub fn load_sxm(sxm_file: SXM, wgpu_state: &RenderState) -> Result<Self> {
        let size = sxm_file.get_image_size()?;
        let scale = sxm_file.get_scan_range()?;
        let translation = sxm_file.get_scan_center()?;
        let transform = Affine2::from_scale_angle_translation(
            Vec2::from(scale) * 1e9,
            0.,
            Vec2::from(translation) * 1e9,
        );
        let scan_image = ScanImage::new(wgpu_state, size, size[1], transform, |data_mut| {
            data_mut.copy_from_slice(&sxm_file.data[0][0]);
        });
        Ok(Self {
            image_data: scan_image,
            image_src: sxm_file,
        })
    }
}

fn image_list_item(image: &StaticImage) -> Atoms<'_> {
    let name = match image.image_src.get_name() {
        Ok(name) => name,
        Err(e) => {
            error!("{e:#}");
            "unnamed"
        }
    };
    (
        Image::new(egui::include_image!("../assets/scan_image_icon.png"))
            .fit_to_exact_size(egui::Vec2::new(20., 20.)),
        name,
    )
        .into_atoms()
}

struct LoadImageModifier(Option<SelectableEntry<StaticImage>>);
impl StateModify<AppState> for LoadImageModifier {
    fn redo(&mut self, state: &mut AppState) -> bool {
        state.image_list.push(self.0.take().unwrap());
        true
    }

    fn undo(&mut self, state: &mut AppState) {
        let mut entry = state.image_list.pop().unwrap();
        entry.selected = false;
        self.0 = Some(entry);
    }
}

struct MoveForwardModifier(Vec<usize>);
impl StateModify<AppState> for MoveForwardModifier {
    fn redo(&mut self, state: &mut AppState) -> bool {
        self.0 = state.image_list.move_indexes_down(&self.0);
        !self.0.is_empty()
    }

    fn undo(&mut self, state: &mut AppState) {
        self.0 = state.image_list.move_indexes_up(&self.0)
    }
}
struct MoveBackwardModifier(Vec<usize>);
impl StateModify<AppState> for MoveBackwardModifier {
    fn redo(&mut self, state: &mut AppState) -> bool {
        self.0 = state.image_list.move_indexes_up(&self.0);
        !self.0.is_empty()
    }

    fn undo(&mut self, state: &mut AppState) {
        self.0 = state.image_list.move_indexes_down(&self.0)
    }
}

#[repr(transparent)]
struct MetersFmt(f64);
impl Display for MetersFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mag = (self.0.abs().log10() / 3.).floor();
        let scaled = self.0 / (10f64).powf(mag * 3.);
        let suf = match mag as i32 {
            4 => Some("Tm"),
            3 => Some("Gm"),
            2 => Some("Mm"),
            1 => Some("km"),
            0 => Some("m"),
            -1 => Some("mm"),
            -2 => Some("μm"),
            -3 => Some("nm"),
            -4 => Some("pm"),
            -5 => Some("fm"),
            _ => None,
        };
        if let Some(suf) = suf {
            warn!("unimplemented `MetersFmt` base for value: `{}`", self.0);
            f64::fmt(&scaled, f)?;
            write!(f, " {}", suf)?;
        } else {
            f64::fmt(&self.0, f)?;
            write!(f, " m")?;
        }
        Ok(())
    }
}
