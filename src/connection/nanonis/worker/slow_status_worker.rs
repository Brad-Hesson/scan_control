use core::f64;
use std::sync::Arc;

use glam::DVec2;
use itertools::Itertools as _;
use nanonis_tcp::{blocking::NanonisTcp, error::NanonisTcpResult};

use crate::connection::{
    nanonis::{channel_state::ChannelState, worker::Worker, ScanStatus},
    shared_state::SharedState,
};

pub struct SlowStatusWorker {
    ctx: egui::Context,
    area_size: SharedState<DVec2>,
    channel_state: SharedState<ChannelState>,
    scan_status: SharedState<ScanStatus>,
}
impl SlowStatusWorker {
    pub fn new(
        ctx: &egui::Context,
        area_size: &SharedState<DVec2>,
        channel_state: &SharedState<ChannelState>,
        scan_status: &SharedState<ScanStatus>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            area_size: area_size.clone(),
            channel_state: channel_state.clone(),
            scan_status: scan_status.clone(),
        }
    }
    fn update_channel_state(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let resp = conn.signals_names_get()?;
        let names = Arc::new(resp.names.into_boxed_slice());
        self.channel_state.modify(|prev| prev.write_names(names));
        Ok(())
    }
    fn update_area_transform(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let piezo_range = conn.piezo_range_get()?;
        let area_transform = DVec2::new(
            piezo_range.range_x as f64 * 1e9,
            piezo_range.range_y as f64 * 1e9,
        );
        if self.area_size.modify_conditional(
            |prev| *prev != area_transform,
            |prev| *prev = area_transform,
        ) {
            self.ctx.request_repaint();
        }
        Ok(())
    }
    fn update_scan_status(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let status = conn.scan_status_get()?;
        if self.scan_status.modify_conditional(
            |prev| prev.scanning != status.running,
            |prev| prev.scanning = status.running,
        ) {
            self.ctx.request_repaint();
        };
        Ok(())
    }
    fn update_channel_opts(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let buffer = conn.scan_buffer_get()?;
        let channels = buffer
            .channel_indexes
            .into_iter()
            .map(|v| v as usize)
            .collect_vec();
        if self.channel_state.peek().options() != channels {
            self.channel_state
                .modify(|prev| prev.modify_opts(|opts| *opts = channels));
            self.ctx.request_repaint();
        }
        Ok(())
    }
}
impl Worker for SlowStatusWorker {
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        self.update_channel_state(conn)?;
        self.update_area_transform(conn)?;
        self.update_scan_status(conn)?;
        self.update_channel_opts(conn)?;
        Ok(())
    }
    fn init(&mut self, _conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        Ok(())
    }

    fn name(&self) -> String {
        "Slow Status Worker".to_string()
    }
}
