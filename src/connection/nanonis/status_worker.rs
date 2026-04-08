use core::f64;
use std::sync::Arc;

use glam::{DAffine2, DVec2};
use itertools::{izip, Itertools as _};
use nanonis_tcp::{
    blocking::NanonisTcp,
    commands::{
        self,
        scan::{FrameGetResponse, FrameSetArgs},
    },
    error::NanonisTcpResult,
};

use crate::connection::{
    nanonis::{channel_state::ChannelState, ScanStatus, Worker},
    shared_state::SharedState,
};

pub struct StatusWorker {
    ctx: egui::Context,
    image_transform: SharedState<DAffine2>,
    area_transform: SharedState<DAffine2>,
    channel_state: SharedState<ChannelState>,
    scan_status: SharedState<ScanStatus>,
}
impl StatusWorker {
    pub fn new(
        ctx: &egui::Context,
        image_transform: &SharedState<DAffine2>,
        area_transform: &SharedState<DAffine2>,
        channel_state: &SharedState<ChannelState>,
        scan_status: &SharedState<ScanStatus>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            image_transform: image_transform.clone(),
            area_transform: area_transform.clone(),
            channel_state: channel_state.clone(),
            scan_status: scan_status.clone(),
        }
    }
    fn update_channel_state(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
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
    fn update_image_transform(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        if let Some(new_transform) = self.image_transform.read_new().as_deref().copied() {
            let frame_args = frame_from_transform(new_transform);
            conn.call::<commands::scan::FrameSet>(&frame_args)?;
            return Ok(());
        }
        let frame = conn.scan_frame_get()?;
        let new_transform = transform_from_frame(&frame);
        if self
            .image_transform
            .modify_conditional(|prev| *prev != new_transform, |val| *val = new_transform)
        {
            self.ctx.request_repaint();
        };
        Ok(())
    }
    fn update_area_transform(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let piezo_range = conn.piezo_range_get()?;
        let area_transform = DAffine2::from_scale(DVec2::new(
            piezo_range.range_x as f64 * 1e9,
            piezo_range.range_y as f64 * 1e9,
        ));
        if self.area_transform.modify_conditional(
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
}
impl Worker for StatusWorker {
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        self.update_image_transform(conn)?;
        self.update_channel_state(conn)?;
        self.update_area_transform(conn)?;
        self.update_scan_status(conn)?;
        Ok(())
    }
    fn init(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let sig_names_resp = conn.signals_names_get()?;
        self.channel_state
            .modify(|state| state.write_names(Arc::new(sig_names_resp.names.into_boxed_slice())));
        Ok(())
    }
}

fn transform_from_frame(frame: &FrameGetResponse) -> DAffine2 {
    DAffine2::from_scale_angle_translation(
        DVec2 {
            x: frame.width as f64 * 1e9,
            y: frame.height as f64 * -1e9,
        },
        frame.angle as f64 / 180. * -f64::consts::PI,
        DVec2 {
            x: frame.center_x as f64 * 1e9,
            y: frame.center_y as f64 * 1e9,
        },
    )
}

fn frame_from_transform(transform: DAffine2) -> FrameSetArgs {
    let (
        DVec2 {
            x: scale_x,
            y: scale_y,
        },
        mut angle,
        DVec2 {
            x: trans_x,
            y: trans_y,
        },
    ) = transform.to_scale_angle_translation();
    if scale_x < 0. {
        angle += f64::consts::PI;
    }
    FrameSetArgs {
        width: (scale_x * 1e-9).abs() as f32,
        height: (scale_y * 1e-9).abs() as f32,
        angle: (angle * 180. / -f64::consts::PI) as f32,
        center_x: (trans_x * 1e-9) as f32,
        center_y: (trans_y * 1e-9) as f32,
    }
}
