use nanonis_tcp::{blocking::NanonisTcp, error::NanonisTcpResult, LineDir, ScanMovementType};

use crate::connection::{
    nanonis::{channel_state::ChannelState, Worker},
    shared_state::SharedState,
};

pub struct LineWorker {
    queue: std::sync::mpsc::Sender<(LineDir, u32)>,
    channel_state: SharedState<ChannelState>,
}
impl LineWorker {
    pub fn new(
        queue: &std::sync::mpsc::Sender<(LineDir, u32)>,
        channel_state: &SharedState<ChannelState>,
    ) -> Self {
        Self {
            queue: queue.clone(),
            channel_state: channel_state.clone(),
        }
    }
}
impl Worker for LineWorker {
    fn init(&mut self, _conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        Ok(())
    }
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let resp = conn.scan_wait_end_of_line(None)?;
        let dir = match resp.movement_type {
            ScanMovementType::Scan(dir) => dir,
            _ => return Ok(()),
        };
        let Some(ch) = self.channel_state.read().selection else {
            return Ok(());
        };
        self.queue.send((dir, ch as u32)).unwrap();
        Ok(())
    }
}
