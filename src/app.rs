use std::any::Any;
use std::path::Path;

use crate::components::file_dialog::ViewportFileDialog;
use crate::scan_view::{BorderRectangle, ImageEncoder, ScanImage, ScanView};
use crate::undo_queue::UndoQueue;
use crate::utils::{SelectableMember, SelectableVecExt as _};
use eframe::egui_wgpu::RenderState;
use egui::{Button, MenuBar, Ui};
use egui::{Color32, Sense};
use egui_file_dialog::FileDialog;
use eyre::{Context, ContextCompat, Result};
use glam::{Affine2, Vec2};
use sxmfile::SXM;
use tracing::{error, info};

pub const COLOR_MAP_SIZE: usize = 256;

pub struct MyApp {
    file_dialog: ViewportFileDialog,
    app_state: AppState,
    image_encoder: ImageEncoder,
    undo_queue: UndoQueue<AppState>, // gradient: egui_colorgradient::Gradient,
                                     // last_gradient: egui_colorgradient::Gradient,
}

pub struct AppState {
    scan_view: ScanView,
    images: Vec<StaticImage>,
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
                images: vec![],
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
            undo_queue: UndoQueue::new(), // last_gradient: gradient.clone(),
                                          // gradient,
        }
    }
    pub fn mod_state<T: Any>(
        &mut self,
        user_data: T,
        redo: impl Fn(&mut AppState, &mut T) + 'static,
        undo: impl Fn(&mut AppState, &mut T) + 'static,
    ) {
        self.undo_queue
            .push(&mut self.app_state, user_data, redo, undo);
    }
    fn load_file(&mut self, wgpu_state: &RenderState, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        info!("Trying to load image `{}`", path.display());
        let sxm_file = sxmfile::SXM::parse_file(&path)?;
        info!("Loaded image `{}`", path.display());
        let static_image = StaticImage::load_sxm(sxm_file, wgpu_state)?;
        static_image
            .image_data
            .write_texture_plane_fit_subtract(wgpu_state, &mut self.image_encoder);
        self.mod_state(
            Some(static_image),
            |state, data| state.images.push(data.take().unwrap()),
            |state, data| *data = Some(state.images.pop().unwrap()),
        );

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
        for img in &mut self.app_state.images {
            img.hovered = false;
        }
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
            if let Some(i) = self.app_state.images.get_selected_index() {
                if i + 1 < self.app_state.images.len() {
                    self.mod_state(
                        (),
                        move |state, _| state.images.swap(i, i + 1),
                        move |state, _| state.images.swap(i, i + 1),
                    );
                }
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::B)) {
            if let Some(i) = self.app_state.images.get_selected_index() {
                if i > 0 {
                    self.mod_state(
                        (),
                        move |state, _| state.images.swap(i, i - 1),
                        move |state, _| state.images.swap(i, i - 1),
                    );
                }
            }
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
        egui::SidePanel::left("list").show(ctx, |ui| {
            for img in &mut self.app_state.images {
                let name = match img
                    .image_src
                    .get_scan_file_path()
                    .and_then(|path| path.file_stem().context("path was not a file"))
                    .and_then(|bytes| {
                        str::from_utf8(bytes).context("file name was not valid utf-8")
                    })
                    .context("failed to get name from file")
                {
                    Ok(name) => name,
                    Err(e) => {
                        error!("{e:#}");
                        "unnamed"
                    }
                };
                let label = ui.add(egui::Label::new(name).sense(Sense::HOVER));
                if label.hovered() {
                    img.hovered = true;
                }
                if img.hovered {
                    label.highlight();
                }
            }
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
                        let mut selected = None;
                        for (i, image) in self.app_state.images.iter_mut().enumerate() {
                            let resp = image.image_data.show(ctx);
                            if resp.clicked() {
                                selected = Some(i);
                            }
                            if resp.hovered() {
                                image.hovered = true;
                            }
                        }
                        if selected.is_some() {
                            self.app_state.images.set_selected_idx(selected);
                        }
                        BorderRectangle {
                            transform: self.app_state.current_scan.transform,
                            color: Color32::RED,
                        }
                        .show(ctx);
                        self.app_state.current_scan.show(ctx);
                        for img in &self.app_state.images {
                            if img.hovered {
                                BorderRectangle {
                                    transform: img.image_data.transform,
                                    color: Color32::LIGHT_BLUE,
                                }
                                .show(ctx);
                            }
                        }
                        if let Some(img) = self.app_state.images.get_selected() {
                            BorderRectangle {
                                transform: img.image_data.transform,
                                color: Color32::GREEN,
                            }
                            .show(ctx);
                        }
                    })
                    .clicked()
                {
                    self.app_state.images.set_selected_idx(None);
                };
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

struct StaticImage {
    uuid: uuid::Uuid,
    image_data: ScanImage,
    image_src: SXM,
    selected: bool,
    hovered: bool,
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
            uuid: uuid::Uuid::new_v4(),
            image_data: scan_image,
            image_src: sxm_file,
            selected: false,
            hovered: false,
        })
    }
    pub fn set_hovered(){
        
    }
}
impl SelectableMember for StaticImage {
    fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}
impl PartialEq for StaticImage {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}
impl Eq for StaticImage {}
