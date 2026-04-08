use crate::components::file_dialog::ViewportFileDialog;
use crate::components::file_tree_extern::ImageTree as FileTree;
use crate::components::selectable_list::{SelectableEntry, SelectableList};
use crate::connection::nanonis::NanonisConnection;
use crate::connection::{LiveImage, ScanArea};
use crate::scan_view::{static_image::StaticImage, BorderRectangle, ImageEncoder, ScanView};
use crate::scan_view::{FileImage, GDSImage, ScaleBar};
use crate::undo_queue::{StateModify, UndoQueue};
use crate::utils::response_group::ResponseGroupExt as _;
use egui::Color32;
use egui::{
    widgets, Align2, Atoms, Button, Frame, Image, IntoAtoms, Layout, MenuBar, Modifiers, Shadow,
    ThemePreference, Ui,
};
use egui_file_dialog::FileDialog;
use glam::{Affine2, DAffine2, DMat3, Mat2, Mat3};
use itertools::{izip, Itertools};
use tracing::error;
use uuid::Uuid;

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
    connection: NanonisConnection,
    file_tree: FileTree,
    file_image_list: SelectableList<FileImage>,
    test_gds: GDSImage,
    scale_bar: ScaleBar,
    scan_area: Option<ScanArea>,
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

        let image_encoder = ImageEncoder::new(wgpu);
        let current_scan = NanonisConnection::new(cc.egui_ctx.clone(), "localhost");
        let file_tree = FileTree::new(image_encoder.clone());
        let mut file_image_list = SelectableList::new();
        let test_image = FileImage::new((), &image_encoder, "IMG_2163.JPEG", DMat3::IDENTITY);
        file_image_list.push(SelectableEntry::new((), test_image, |img| {
            "img".into_atoms()
        }));
        let test_gds = GDSImage::new(
            &image_encoder,
            "As_Implanted_MLA150.GDS",
            DAffine2::IDENTITY,
        );
        Self {
            app_state: AppState {
                scan_view: ScanView::new(&image_encoder),
                image_list: SelectableList::new(),
                connection: current_scan,
                file_tree,
                file_image_list,
                test_gds,
                scale_bar: ScaleBar::new(),
                scan_area: None,
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
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match &mut self.app_state.scan_area {
            None => {
                self.app_state.scan_area = self
                    .app_state
                    .connection
                    .poll_connected(&self.image_encoder);
            }
            Some(live_image) => {
                self.app_state
                    .connection
                    .update_live_image(live_image, &self.image_encoder);
            }
        }
        // if let Some(mut new_image) = self.app_state.connection.update(&self.image_encoder) {
        //     let mut new_name_num = 0;
        //     let mut new_name = new_image.name.clone();
        //     while self
        //         .app_state
        //         .image_list
        //         .iter()
        //         .any(|entry| entry.name == new_name)
        //     {
        //         new_name_num += 1;
        //         new_name = format!("{}({})", new_image.name, new_name_num);
        //     }
        //     new_image.name = new_name;
        //     let entry = SelectableEntry::new(Uuid::new_v4(), new_image, |image| {
        //         (&image.name).into_atoms()
        //     });
        //     self.app_state.image_list.push(entry);
        // }
        if let Some(paths) = self.file_dialog.take_picked_multiple() {
            // self.load_files(paths)
            //     .context("file load failed")
            //     .ok_trace();
        }
        if let Some(path) = self.folder_dialog.take_picked() {
            self.app_state.file_tree.load_dir(&path).ok_trace();
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
                let image_resp = self.app_state.scan_view.show(ui, |ui| {
                    for img in self.app_state.file_image_list.iter_mut() {
                        img.show(ui);
                    }
                    let files = &mut self.app_state.image_list;
                    for i in 0..files.len() {
                        let resp = files[i].show(ui).synchronize(&mut files[i].resp_group);
                        if resp.orig.clicked() {
                            if ui.input(|i| i.modifiers.ctrl) {
                                files[i].selected = !files[i].selected;
                            } else {
                                files.clear_selected();
                                files[i].selected = true;
                            }
                        }
                        if resp.sync.hovered() {
                            BorderRectangle {
                                transform: files[i].transform,
                                color: Color32::LIGHT_BLUE,
                                dashed: false,
                            }
                            .show(ui);
                        }
                        if files[i].selected {
                            BorderRectangle {
                                transform: files[i].transform,
                                color: Color32::GREEN,
                                dashed: false,
                            }
                            .show(ui);
                        }
                    }
                    if let Some(scan_area) = &mut self.app_state.scan_area {
                        scan_area.show(ui);
                    }
                    if let Some(image) = files.get_hovered(ui.ctx()) {
                        BorderRectangle {
                            transform: image.transform,
                            color: Color32::LIGHT_BLUE,
                            dashed: true,
                        }
                        .show(ui);
                    }
                    for image in files.iter_selected() {
                        BorderRectangle {
                            transform: image.transform,
                            color: Color32::GREEN,
                            dashed: true,
                        }
                        .show(ui);
                    }
                    self.app_state.test_gds.show(ui);
                    self.app_state.scale_bar.show(ui);
                });
                if image_resp.clicked() {
                    self.app_state.image_list.clear_selected();
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
                        // self.app_state.file_tree.show(ui);
                        self.app_state.image_list.show(ui);
                    });
                let mut new_top = ui.clip_rect().top();
                if let Some(scan_area) = &mut self.app_state.scan_area {
                    new_top = egui::Window::new("Current Scan")
                        .frame(
                            Frame::window(&ctx.style())
                                .multiply_with_opacity(0.5)
                                .shadow(Shadow::NONE),
                        )
                        .constrain_to(ui.min_rect())
                        .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                        .resizable(false)
                        .show(ctx, |ui| {
                            let vis = &mut ui.style_mut().visuals.widgets.inactive;
                            vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
                            scan_area.show_menu(ui, &self.image_encoder);
                        })
                        .unwrap()
                        .response
                        .rect
                        .bottom();
                }
                let selected = self
                    .app_state
                    .image_list
                    .iter_selected_indexes()
                    .collect_vec();
                for i in selected {
                    let name = &self.app_state.image_list[i].name;
                    let mut rect = ui.min_rect();
                    rect.set_top(new_top);
                    new_top = egui::Window::new(name)
                        .frame(Frame::window(&ctx.style()).multiply_with_opacity(0.5))
                        .constrain_to(rect)
                        .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                        .resizable(false)
                        .show(ctx, |ui| {
                            self.app_state.image_list[i]
                                .show_image_menu(ui, &mut self.image_encoder);
                        })
                        .unwrap()
                        .response
                        .rect
                        .bottom();
                }
                for i in 0..self.app_state.file_image_list.len() {
                    let name = &self.app_state.file_image_list[i].name;
                    let mut rect = ui.min_rect();
                    rect.set_top(new_top);
                    new_top = egui::Window::new(name)
                        .frame(Frame::window(&ctx.style()).multiply_with_opacity(0.5))
                        .constrain_to(rect)
                        .anchor(Align2::RIGHT_TOP, egui::Vec2::new(5., 5.))
                        .resizable(false)
                        .show(ctx, |ui| {
                            self.app_state.file_image_list[i].show_menu(ui);
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

fn image_list_item(image: &StaticImage) -> Atoms<'_> {
    let name = &image.name;
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

pub trait OkTraceExt<T> {
    fn ok_trace(self) -> Option<T>;
}
impl<T, E: std::fmt::Display> OkTraceExt<T> for std::result::Result<T, E> {
    fn ok_trace(self) -> Option<T> {
        self.inspect_err(|e| error!("{e:#}")).ok()
    }
}
