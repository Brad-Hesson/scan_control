use std::path::PathBuf;

use crossbeam::channel::TryRecvError;

use crate::{scan_view::ImageEncoder, view_object};

pub struct ObjectImportDialog {
    channel_tx: crossbeam::channel::Sender<view_object::Object>,
    channel_rx: crossbeam::channel::Receiver<view_object::Object>,
    dialog: rfd::FileDialog,
}
impl ObjectImportDialog {
    pub fn new() -> Self {
        let dialog = rfd::FileDialog::new()
            .add_filter("Any", &["png", "jpeg", "gds"])
            .add_filter("Image", &["png", "jpeg"])
            .add_filter("GDS", &["gds"])
            .set_title("Import Files");
        let (channel_tx, channel_rx) = crossbeam::channel::unbounded();
        Self {
            channel_tx,
            channel_rx,
            dialog,
        }
    }
    pub fn pick_files(&mut self, encoder: ImageEncoder) {
        let dialog = self.dialog.clone();
        let channel = self.channel_tx.clone();
        std::thread::spawn(move || {
            if let Some(paths) = dialog.pick_files() {
                for path in paths {
                    if let Some(object) = view_object::Object::import(path, &encoder) {
                        channel.send(object).unwrap();
                    }
                }
            }
        });
    }
    pub fn try_recv_object(&mut self) -> Option<view_object::Object> {
        match self.channel_rx.try_recv() {
            Ok(path) => Some(path),
            Err(_) => None,
        }
    }
}
