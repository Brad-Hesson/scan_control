use std::sync::Arc;

use nanonis_tcp::{
    blocking::NanonisTcp, commands::scan::FrameDataGrabResponse, error::NanonisTcpResult, LineDir,
};

use crate::connection::{live_image::FrameData, nanonis::Worker, shared_state::SharedState};

pub struct FrameWorker {
    ctx: egui::Context,
    forward_data: SharedState<FrameData>,
    backward_data: SharedState<FrameData>,
    queue: std::sync::mpsc::Receiver<(LineDir, u32)>,
}
impl FrameWorker {
    pub fn new(
        ctx: &egui::Context,
        forward_data: &SharedState<FrameData>,
        backward_data: &SharedState<FrameData>,
        queue: std::sync::mpsc::Receiver<(LineDir, u32)>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            forward_data: forward_data.clone(),
            backward_data: backward_data.clone(),
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
            let mut frame: FrameData = conn.scan_frame_data_grab(ch, dir)?.into();
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
