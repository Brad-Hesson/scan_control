use std::sync::Arc;

use nanonis_tcp::{
    blocking::NanonisTcp, commands::scan::FrameDataGrabResponse, error::NanonisTcpResult, LineDir,
};

use crate::connection::{
    live_image::FrameData,
    nanonis::{ScanStatus, Worker},
    shared_state::SharedState,
};

pub struct FrameWorker {
    ctx: egui::Context,
    forward_data: SharedState<FrameData>,
    backward_data: SharedState<FrameData>,
    queue: std::sync::mpsc::Receiver<(LineDir, u32)>,
    scan_status: SharedState<ScanStatus>,
}
impl FrameWorker {
    pub fn new(
        ctx: &egui::Context,
        forward_data: &SharedState<FrameData>,
        backward_data: &SharedState<FrameData>,
        queue: std::sync::mpsc::Receiver<(LineDir, u32)>,
        scan_status: &SharedState<ScanStatus>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            forward_data: forward_data.clone(),
            backward_data: backward_data.clone(),
            scan_status: scan_status.clone(),
            queue,
        }
    }
}
impl Worker for FrameWorker {
    fn init(&mut self, _conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        Ok(())
    }

    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        if let Ok((dir, ch)) = self.queue.recv() {
            let resp = conn.scan_frame_data_grab(ch, dir)?;
            self.scan_status.modify_conditional(
                |prev| prev.scan_dir != resp.scan_dir,
                |prev| prev.scan_dir = resp.scan_dir,
            );
            let mut frame = FrameData::from(resp);
            if (frame.size[0] * frame.size[1]) == 0 {
                frame = FrameData::default();
            }
            match dir {
                LineDir::Forward => self.forward_data.write(frame),
                LineDir::Backward => self.backward_data.write(frame),
            }
            self.ctx.request_repaint();
        }
        Ok(())
    }
    fn name(&self) -> String {
        "Frame Downloader".to_string()
    }
}

impl From<FrameDataGrabResponse> for FrameData {
    fn from(frame: FrameDataGrabResponse) -> Self {
        Self {
            size: [
                frame.scan_data.size[1] as u32,
                frame.scan_data.size[0] as u32,
            ],
            data: Arc::new(frame.scan_data.data.into_boxed_slice()),
        }
    }
}
