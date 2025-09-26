use std::fmt::Display;
use std::path::{Path, PathBuf};

use crate::components::file_dialog::ViewportFileDialog;
use crate::components::file_tree::FileTree;
use crate::components::selectable_list::{SelectableEntry, SelectableList};
use crate::scan_view::{BorderRectangle, ImageEncoder, ScanImage, ScanView};
use crate::undo_queue::{StateModify, UndoQueue};
use crate::utils::response_group::ResponseGroupExt as _;
use egui::{
    widgets, Align2, Atoms, Button, Frame, Image, IntoAtoms, Layout, MenuBar, Modifiers, Shadow,
    ThemePreference, Ui,
};
use egui::{Color32, ComboBox};
use egui_file_dialog::FileDialog;
use eyre::{Context, Result};
use glam::{Affine2, Vec2};
use image_compute::image_compute::{FitData, FitType};
use itertools::{izip, Itertools};
use sxmfile::SXM;
use tracing::{error, info, warn};

pub const COLOR_MAP_SIZE: usize = 256;

pub struct MyApp {
    file_dialog: ViewportFileDialog,
    app_state: AppState,
    image_encoder: ImageEncoder,
    undo_queue: UndoQueue<AppState>,
    current_theme: ThemePreference,
    folder_dialog: ViewportFileDialog,
}

pub struct AppState {
    scan_view: ScanView,
    image_list: SelectableList<StaticImage>,
    current_scan: StaticImage,
    file_tree: FileTree<StaticImage>,
}

// trait UnwrapTraceExt{
//     fn unwrap_trace<T>(self) -> Option<T>;
// }
// impl<T> UnwrapTraceExt for Result<T>{
//     fn unwrap_trace<T>(self) -> Option<T> {
//         todo!()
//     }
// }

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

        let src_sxm = SXM::parse_file("data/20240229_075.sxm").unwrap();
        let mut image_encoder = ImageEncoder::new(wgpu);
        let current_scan = StaticImage::load_sxm(src_sxm, &image_encoder).unwrap();
        current_scan.image_data.clear(&mut image_encoder);
        let enc = image_encoder.clone();
        let load_fn = move |path: &Path| {
            if !path.extension().is_some_and(|ext| ext == "sxm") {
                return None;
            }
            let sxm = SXM::parse_file(path)
                .with_context(|| format!("failed to load `{}`", path.display()))
                .ok_trace()?;
            let out = StaticImage::load_sxm(sxm, &enc).ok_trace()?;
            out.update_texture(&enc);
            Some(out)
        };
        let file_tree = FileTree::new(&cc.egui_ctx, load_fn).unwrap();
        Self {
            app_state: AppState {
                scan_view: ScanView::new(&image_encoder),
                image_list: SelectableList::new(),
                current_scan,
                file_tree,
            },
            image_encoder,
            file_dialog: ViewportFileDialog::new(FileDialog::new().title("Import File")),
            undo_queue: UndoQueue::new(),
            current_theme: cc.egui_ctx.theme().into(),
            folder_dialog: ViewportFileDialog::new(FileDialog::new().title("Open folder")),
        }
    }
    pub fn mod_state<T: StateModify<AppState>>(&mut self, modifier: T) {
        self.undo_queue.push(&mut self.app_state, modifier);
    }
    fn load_files(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        let mut imgs = vec![];
        for path in paths {
            info!("Trying to load image `{}`", path.display());
            let sxm_file = sxmfile::SXM::parse_file(&path)?;
            info!("Loaded image `{}`", path.display());
            let static_image = StaticImage::load_sxm(sxm_file, &self.image_encoder)?;
            static_image.update_texture(&mut self.image_encoder);
            let entry = SelectableEntry::new(static_image, image_list_item);
            imgs.push(entry);
        }
        imgs.sort_by_cached_key(|img| img.image_src.get_datetime().unwrap());
        self.mod_state(LoadImagesModifier::new(imgs));
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(paths) = self.file_dialog.take_picked_multiple() {
            self.load_files(paths)
                .context("file load failed")
                .ok_trace();
        }
        if let Some(path) = self.folder_dialog.take_picked() {
            self.app_state.file_tree.load_path(&path).ok_trace();
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
            && !self.app_state.current_scan.image_data.is_full()
        {
            let current_scan = &mut self.app_state.current_scan;
            let [x, y] = current_scan.image_data.current_size();
            let line = &current_scan.image_src.data[0][0][x as usize * y as usize..][..x as usize];
            current_scan
                .image_data
                .write_line(&self.image_encoder, line)
                .unwrap();
            current_scan.update_texture(&mut self.image_encoder);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::C)) {
            self.app_state
                .current_scan
                .image_data
                .clear(&mut self.image_encoder);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::Delete)) {
            let idxs = self
                .app_state
                .image_list
                .iter_selected_indexes()
                .collect_vec();
            self.mod_state(DeleteImagesModifier::new(idxs));
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::T)) {
            let new_theme = match self.current_theme {
                ThemePreference::Dark => ThemePreference::Light,
                ThemePreference::Light => ThemePreference::Dark,
                ThemePreference::System => unreachable!(),
            };
            ctx.set_theme(new_theme);
            self.current_theme = new_theme;
        }
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            MenuBar::new().ui(ui, |ui| {
                file_menu_button(ui, ctx, self);
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::global_theme_preference_switch(ui);
                });
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
                        let file_tree = &mut self.app_state.file_tree;
                        for i in file_tree.iter_indexers().rev() {
                            let resp = file_tree[i]
                                .image_data
                                .show(ctx)
                                .synchronize(&mut file_tree[i].resp_group);
                            if resp.orig.clicked() {
                                if ctx.ui.input(|i| i.modifiers.ctrl) {
                                    file_tree[i].selected = !file_tree[i].selected;
                                } else {
                                    file_tree.clear_selected();
                                    file_tree[i].selected = true;
                                }
                            }
                            if resp.sync.hovered() {
                                BorderRectangle {
                                    transform: file_tree[i].image_data.transform,
                                    color: Color32::LIGHT_BLUE,
                                    dashed: false,
                                }
                                .show(ctx);
                            }
                            if file_tree[i].selected {
                                BorderRectangle {
                                    transform: file_tree[i].image_data.transform,
                                    color: Color32::GREEN,
                                    dashed: false,
                                }
                                .show(ctx);
                            }
                        }
                        self.app_state.current_scan.image_data.show(ctx);
                        BorderRectangle {
                            transform: self.app_state.current_scan.image_data.transform,
                            color: Color32::RED,
                            dashed: false,
                        }
                        .show(ctx);
                        if let Some(image) = file_tree.get_hovered(ctx.ui.ctx()) {
                            BorderRectangle {
                                transform: image.image_data.transform,
                                color: Color32::LIGHT_BLUE,
                                dashed: true,
                            }
                            .show(ctx);
                        }
                        for image in file_tree.iter_selected() {
                            BorderRectangle {
                                transform: image.image_data.transform,
                                color: Color32::GREEN,
                                dashed: true,
                            }
                            .show(ctx);
                        }
                    })
                    .clicked()
                {
                    self.app_state.file_tree.clear_selected();
                };
                egui::Window::new("Images")
                    .frame(
                        Frame::window(&ctx.style())
                            .multiply_with_opacity(0.5)
                            .shadow(Shadow::NONE),
                    )
                    .constrain_to(ui.min_rect())
                    .anchor(Align2::LEFT_TOP, egui::Vec2::new(5., 5.))
                    .default_size([200., 400.])
                    .resizable(true)
                    .scroll([false, true])
                    .show(ctx, |ui| {
                        let vis = &mut ui.style_mut().visuals.widgets.inactive;
                        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
                        // self.app_state.image_list.show(ui);
                        self.app_state.file_tree.show(ui);
                    });
                let mut new_top = egui::Window::new("Current Scan")
                    .frame(
                        Frame::window(&ctx.style())
                            .multiply_with_opacity(0.5)
                            .shadow(Shadow::NONE),
                    )
                    .constrain_to(ui.min_rect())
                    .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                    .resizable(false)
                    .show(ctx, |ui| {
                        image_menu(
                            ui,
                            &mut self.app_state.current_scan,
                            &mut self.image_encoder,
                        )
                    })
                    .unwrap()
                    .response
                    .rect
                    .bottom();
                for i in self
                    .app_state
                    .image_list
                    .iter_selected_indexes()
                    .rev()
                    .collect_vec()
                    .into_iter()
                {
                    let name = self.app_state.image_list[i]
                        .image_src
                        .get_name()
                        .ok_trace()
                        .unwrap_or("unnamed");
                    let mut rect = ui.min_rect();
                    rect.set_top(new_top);
                    new_top = egui::Window::new(name)
                        .frame(Frame::window(&ctx.style()).multiply_with_opacity(0.5))
                        .constrain_to(rect)
                        .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                        .resizable(false)
                        .show(ctx, |ui| {
                            image_menu(
                                ui,
                                &mut self.app_state.image_list[i],
                                &mut self.image_encoder,
                            )
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
        if ui.add(Button::new("Import File")).clicked() {
            app.file_dialog.pick_multiple();
        }
        if ui.add(Button::new("Open Folder")).clicked() {
            app.folder_dialog.pick_directory();
        }
    });
    app.file_dialog.update(ctx);
    app.folder_dialog.update(ctx);
}
fn image_menu(ui: &mut Ui, image: &mut StaticImage, image_encoder: &mut ImageEncoder) {
    let vis = &mut ui.style_mut().visuals.widgets.inactive;
    vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
    let types = [
        (FitType::LineFitSubtract, "Line Fit"),
        (FitType::LineMeanSubtract, "Line Mean Subtract"),
        (FitType::PlaneFitSubtract, "Plane Fit"),
        (FitType::MeanSubtract, "Mean Subtract"),
    ];
    let sub_type_selector = ComboBox::new((image.image_data.uuid(), "fit type"), "")
        .selected_text(types.iter().find(|(t, _)| *t == image.fit_type).unwrap().1)
        .show_ui(ui, |ui| {
            ui.set_min_height(82.);
            let changed = types
                .iter()
                .copied()
                .map(|(typ, name)| {
                    ui.selectable_value(&mut image.fit_type, typ, name)
                        .clicked()
                })
                .any(|b| b);
            changed
        });
    if matches!(sub_type_selector.inner, Some(true)) {
        image.update_texture(image_encoder);
    }
    if let Some(data) = image.image_data.norm_data.read().as_ref() {
        ui.label(format!("Min: {:.2}", MetersFmt(data.min)));
        ui.label(format!("Max: {:.2}", MetersFmt(data.max)));
        ui.label(format!("Std Dev: {:.2}", MetersFmt(data.stddev)));
    }
    if let Some(data) = image.image_data.fit_data.read().as_ref() {
        match data {
            FitData::PlaneFitSubtract {
                mean,
                x_slope,
                y_slope,
            } => {
                ui.label(format!("Mean: {:.2}", MetersFmt(*mean)));
                ui.label(format!("X Slope: {:.2}", MetersFmt(*x_slope)));
                ui.label(format!("Y Slope: {:.2}", MetersFmt(*y_slope)));
            }
            FitData::MeanSubtract { mean } => {
                ui.label(format!("Mean: {:.2}", MetersFmt(*mean)));
            }
            FitData::LineMeanSubtract { means } => {
                for m in means {
                    ui.label(format!("{:.2}", MetersFmt(*m)));
                }
            }
            FitData::LineFitSubtract { means, slopes } => {
                for (m, s) in izip!(means, slopes) {
                    ui.label(format!("{:.2}  {:.2}", MetersFmt(*m), MetersFmt(*s)));
                }
            }
        }
    }
}

struct StaticImage {
    image_data: ScanImage,
    image_src: SXM,
    fit_type: FitType,
}
impl StaticImage {
    pub fn load_sxm(sxm_file: SXM, image_encoder: &ImageEncoder) -> Result<Self> {
        let size = sxm_file.get_image_size()?;
        let scale = sxm_file.get_scan_range()?;
        let translation = sxm_file.get_scan_center()?;
        let transform = Affine2::from_scale_angle_translation(
            Vec2::from(scale) * 1e9,
            0.,
            Vec2::from(translation) * 1e9,
        );
        let scan_image = ScanImage::new(image_encoder, size, size[1], transform, |data_mut| {
            data_mut.copy_from_slice(&sxm_file.data[0][0]);
        });
        Ok(Self {
            image_data: scan_image,
            image_src: sxm_file,
            fit_type: FitType::LineFitSubtract,
        })
    }
    pub fn update_texture(&self, image_encoder: &ImageEncoder) {
        self.image_data.write_texture(image_encoder, self.fit_type);
    }
}

fn image_list_item(image: &StaticImage) -> Atoms<'_> {
    let name = image.image_src.get_name().ok_trace().unwrap_or("unnamed");
    (
        Image::new(egui::include_image!("../assets/scan_image_icon.png")),
        name,
    )
        .into_atoms()
}

struct DeleteImagesModifier {
    imgs: Vec<SelectableEntry<StaticImage>>,
    idxs: Vec<usize>,
}
impl DeleteImagesModifier {
    pub fn new(idxs: Vec<usize>) -> Self {
        Self {
            imgs: Vec::with_capacity(idxs.len()),
            idxs,
        }
    }
}
impl StateModify<AppState> for DeleteImagesModifier {
    fn redo(&mut self, state: &mut AppState) -> bool {
        for idx in self.idxs.iter().rev() {
            self.imgs.push(state.image_list.remove(*idx));
        }
        true
    }
    fn undo(&mut self, state: &mut AppState) {
        for (idx, img) in izip!(&self.idxs, self.imgs.drain(..).rev()) {
            state.image_list.insert(*idx, img);
        }
    }
}

struct LoadImagesModifier {
    imgs: Vec<SelectableEntry<StaticImage>>,
    num: usize,
}
impl LoadImagesModifier {
    pub fn new(imgs: Vec<SelectableEntry<StaticImage>>) -> Self {
        Self {
            num: imgs.len(),
            imgs,
        }
    }
}
impl StateModify<AppState> for LoadImagesModifier {
    fn redo(&mut self, state: &mut AppState) -> bool {
        state.image_list.extend(self.imgs.drain(..));
        true
    }
    fn undo(&mut self, state: &mut AppState) {
        let start = state.image_list.len() - self.num;
        self.imgs.extend(state.image_list.drain(start..));
        for img in &mut self.imgs {
            img.selected = false;
        }
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
            f64::fmt(&scaled, f)?;
            write!(f, " {}", suf)?;
        } else if self.0 == 0. {
            f64::fmt(&self.0, f)?;
            write!(f, " m")?;
        } else {
            warn!("unimplemented `MetersFmt` base for value: `{}`", self.0);
            f64::fmt(&self.0, f)?;
            write!(f, " m")?;
        }
        Ok(())
    }
}

trait OkTraceExt<T> {
    fn ok_trace(self) -> Option<T>;
}
impl<T, E: std::fmt::Display> OkTraceExt<T> for std::result::Result<T, E> {
    fn ok_trace(self) -> Option<T> {
        self.inspect_err(|e| error!("{e:#}")).ok()
    }
}
