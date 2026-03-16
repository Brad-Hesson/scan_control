use std::sync::mpsc::{Receiver, Sender};

use nanonis_tcp::{commands::scan::FrameDataGrabResponse, nonblocking::NanonisTcp, LineDir};
use tokio::net::ToSocketAddrs;

use crate::scan_view::{ImageEncoder, NanonisImage, ScanImage, StaticImage};

pub struct NanonisConnection {
    pub live_image: NanonisImage,
    stamp_rx: Receiver<FrameDataGrabResponse>,
    pub num_images: usize,
}

impl NanonisConnection {
    pub fn new(ctx: egui::Context, image_encoder: &ImageEncoder, address: impl AsRef<str>) -> Self {
        let (stamp_tx, stamp_rx) = std::sync::mpsc::channel();
        tokio::spawn(scan_worker((address.as_ref().to_string(), 6504), stamp_tx, ctx.clone()));
        let live_image = NanonisImage::new(ctx, image_encoder);
        Self {
            stamp_rx,
            live_image,
            num_images: 0,
        }
    }
    pub fn get_stamps(&mut self, encoder: &ImageEncoder) -> Vec<StaticImage> {
        let mut imgs = Vec::new();
        while let Ok(frame) = self.stamp_rx.try_recv() {
            println!("loading stamp");
            let image_data = ScanImage::new(
                encoder,
                self.live_image.image_data.size(),
                self.live_image.image_data.transform,
                self.live_image.image_data.norm_type,
                |buf| buf.copy_from_slice(&frame.scan_data.data),
            );
            image_data.write_texture(encoder, self.live_image.fit_type);
            let name = format!("image({})", self.num_images);
            self.num_images += 1;
            let image = StaticImage {
                image_data,
                fit_type: self.live_image.fit_type,
                norm_type: self.live_image.norm_type,
                std_dev: self.live_image.std_dev,
                name,
            };
            imgs.push(image);
        }
        imgs
    }
}

async fn scan_worker(
    addr: impl ToSocketAddrs,
    tx: Sender<FrameDataGrabResponse>,
    ctx: egui::Context,
) {
    let mut conn = NanonisTcp::new(addr).await.unwrap();
    loop {
        conn.scan_wait_end_of_scan(None).await.unwrap();
        let frame = conn
            .scan_frame_data_grab(30, LineDir::Forward)
            .await
            .unwrap();
        tx.send(frame).unwrap();
        ctx.request_repaint();
    }
}
