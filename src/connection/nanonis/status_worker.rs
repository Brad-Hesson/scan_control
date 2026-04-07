use std::sync::Arc;

use glam::DAffine2;
use itertools::{izip, Itertools as _};
use nanonis_tcp::{
    blocking::NanonisTcp, commands::scan::FrameGetResponse, error::NanonisTcpResult,
};

use crate::connection::{
    nanonis::{channel_state::ChannelState, Worker},
    shared_state::SharedState,
};

pub struct StatusWorker {
    ctx: egui::Context,
    transform: SharedState<DAffine2>,
    channel_state: SharedState<ChannelState>,
}
impl StatusWorker {
    pub fn new(
        ctx: &egui::Context,
        transform: &SharedState<DAffine2>,
        channel_state: &SharedState<ChannelState>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            transform: transform.clone(),
            channel_state: channel_state.clone(),
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
}
impl Worker for StatusWorker {
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let frame = conn.scan_frame_get()?;
        let new_transform = transform_from_frame(&frame);
        if self.transform.modify_conditional(
            |prev| izip!(prev.to_cols_array(), new_transform.to_cols_array()).any(|(a, b)| a != b),
            |val| *val = new_transform,
        ) {
            self.ctx.request_repaint();
        };
        self.update_channel_state(conn)?;
        // let props = conn.scan_props_get()?;
        // if self.name.modify_conditional(
        //     |prev| *prev != props.series_name,
        //     |val| *val = props.series_name.clone(),
        // ) {
        //     self.ctx.request_repaint();
        // };
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
        [frame.width as f64 * 1e9, frame.height as f64 * -1e9].into(),
        frame.angle as f64 / 180. * -3.14,
        [frame.center_x as f64 * 1e9, frame.center_y as f64 * 1e9].into(),
    )
}
