use nanonis_tcp::{blocking::NanonisTcp, error::NanonisTcpResult, LineDir, ScanMovementType};
use tracing::trace;

use crate::connection::{
    nanonis::{worker::Worker, OverwriteQueueSender, ScanStatus},
    shared_state::SharedState,
};

pub struct LineWorker {
    queue: OverwriteQueueSender<LineDir>,
    scan_status: SharedState<ScanStatus>,
}
impl LineWorker {
    pub fn new(
        queue: &OverwriteQueueSender<LineDir>,
        scan_status: &SharedState<ScanStatus>,
    ) -> Self {
        Self {
            queue: queue.clone(),
            scan_status: scan_status.clone(),
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
        trace!("line finished {} {:?}", resp.line_number, dir);
        self.scan_status.modify_conditional(
            |prev| prev.line_number != resp.line_number as u32 || prev.line_dir != dir,
            |prev| {
                prev.line_number = resp.line_number as u32;
                prev.scanning = true;
                prev.line_dir = dir;
            },
        );
        self.queue.send(dir);
        Ok(())
    }
    fn name(&self) -> String {
        "Line Waiter".to_string()
    }
}
