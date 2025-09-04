use std::ops::{Deref, DerefMut};

use egui::{ViewportBuilder, ViewportId};
use egui_file_dialog::{DialogState, FileDialog};

pub struct ViewportFileDialog {
    file_dialog: FileDialog,
    builder: ViewportBuilder,
    id: ViewportId,
}
impl ViewportFileDialog {
    pub fn new(mut file_dialog: FileDialog) -> Self {
        Self {
            builder: ViewportBuilder {
                active: Some(true),
                title: file_dialog.config_mut().title.clone(),
                inner_size: Some(file_dialog.config_mut().default_size),
                ..Default::default()
            },
            file_dialog: file_dialog
                .title_bar(false)
                .resizable(false)
                .fixed_pos([1.0, 1.0]),
            id: ViewportId::from_hash_of("picker"),
        }
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        if self.file_dialog.state() == DialogState::Open {
            ctx.show_viewport_immediate(
                self.id,
                self.builder.clone(),
                move |ctx, _viewport_class| {
                    egui::CentralPanel::default().show(ctx, |_ui| {
                        let rect = _ui.available_rect_before_wrap();
                        let width = rect.width();
                        let height = rect.height();
                        let config = self.file_dialog.config_mut();
                        config.min_size = [width, height].into();
                        config.max_size = Some([width, height].into());
                        self.file_dialog.update(ctx);
                    });
                    if ctx.input(|input| input.viewport().close_requested()) {
                        let storage = self.file_dialog.storage_mut().clone();
                        let config = self.file_dialog.config_mut().clone();
                        self.file_dialog = FileDialog::new().storage(storage);
                        *self.file_dialog.config_mut() = config;
                    }
                },
            );
        }
    }
}
impl Deref for ViewportFileDialog {
    type Target = FileDialog;

    fn deref(&self) -> &Self::Target {
        &self.file_dialog
    }
}
impl DerefMut for ViewportFileDialog {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.file_dialog
    }
}
