use std::{
    f64, i32,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use glam::{DVec2, IVec2};
use itertools::Itertools as _;
use nanonis_tcp::{
    blocking::NanonisTcp,
    error::{NanonisTcpError, NanonisTcpResult},
    MotorAxis, MotorDir,
};
use tracing::debug;

use crate::connection::{
    nanonis::{
        channel_state::ChannelState, command_channel::CommandChannelReciever, worker::Worker,
        ScanStatus,
    },
    shared_state::SharedState,
};

const DEBUG_COURSE: bool = false;

pub struct SlowStatusWorker {
    ctx: egui::Context,
    area_size: SharedState<DVec2>,
    channel_state: SharedState<ChannelState>,
    scan_status: SharedState<ScanStatus>,
    base_name: SharedState<String>,
    course_voltages: SharedState<DVec2>,
    course_reciever: CommandChannelReciever<(IVec2, u32), ()>,
    init: Arc<AtomicBool>,
}
impl SlowStatusWorker {
    pub fn new(
        ctx: &egui::Context,
        area_size: &SharedState<DVec2>,
        channel_state: &SharedState<ChannelState>,
        scan_status: &SharedState<ScanStatus>,
        base_name: &SharedState<String>,
        course_voltages: &SharedState<DVec2>,
        course_reciever: &CommandChannelReciever<(IVec2, u32), ()>,
        init: &Arc<AtomicBool>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            area_size: area_size.clone(),
            channel_state: channel_state.clone(),
            scan_status: scan_status.clone(),
            base_name: base_name.clone(),
            course_voltages: course_voltages.clone(),
            course_reciever: course_reciever.clone(),
            init: init.clone(),
        }
    }
    fn execute_course_move(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        if let Some((steps, group)) = self.course_reciever.try_recv() {
            let args = move_args_from_steps(steps);
            debug!("Executing {args:?} on group index {group}");
            let result = if DEBUG_COURSE {
                std::thread::sleep(Duration::from_secs(2));
                Ok(())
            } else {
                || -> NanonisTcpResult<()> {
                    if let Some((dir, steps)) = args[0] {
                        conn.motor_start_move(dir, steps, group, true)?;
                    }
                    if let Some((dir, steps)) = args[1] {
                        conn.motor_start_move(dir, steps, group, true)?;
                    }
                    Ok(())
                }()
            };
            self.course_reciever.send_response(());
            result?;
        }
        Ok(())
    }
    fn update_signal_names(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let resp = conn.signals_names_get()?;
        if self.channel_state.peek().signal_names() != resp.names {
            let names = Arc::new(resp.names.into_boxed_slice());
            self.channel_state
                .modify(|prev| prev.write_signal_names(names));
        }
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
    fn update_base_name(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let frame_meta = conn.scan_props_get()?;
        if self.base_name.modify_conditional(
            |prev| *prev != frame_meta.series_name,
            |prev| *prev = frame_meta.series_name.clone(),
        ) {
            self.ctx.request_repaint();
        }
        Ok(())
    }
    fn update_course_voltages(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        let err_pred = |e: &NanonisTcpError| matches!(e, NanonisTcpError::Api(_));
        let default = if DEBUG_COURSE { 1. } else { 0. };
        let x_data = conn
            .motor_freq_amp_get(MotorAxis::X)
            .map(|resp| resp.amplitude as f64)
            .or_else(|e| err_pred(&e).then_some(default).ok_or(e))?;
        let y_data = conn
            .motor_freq_amp_get(MotorAxis::Y)
            .map(|resp| resp.amplitude as f64)
            .or_else(|e| err_pred(&e).then_some(default).ok_or(e))?;
        let new_data = DVec2::new(x_data, y_data);
        self.course_voltages
            .modify_conditional(|prev| *prev != new_data, |prev| *prev = new_data);
        Ok(())
    }
}
impl Worker for SlowStatusWorker {
    fn work(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        self.update_area_transform(conn)?;
        self.update_scan_status(conn)?;
        self.update_channel_opts(conn)?;
        self.update_base_name(conn)?;
        self.update_course_voltages(conn)?;
        self.execute_course_move(conn)?;
        Ok(())
    }
    fn init(&mut self, conn: &mut NanonisTcp) -> NanonisTcpResult<()> {
        self.update_signal_names(conn)?;
        self.update_area_transform(conn)?;
        self.update_scan_status(conn)?;
        self.update_channel_opts(conn)?;
        self.update_base_name(conn)?;
        self.update_course_voltages(conn)?;
        self.init.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> String {
        "Slow Status Worker".to_string()
    }
}

fn move_args_from_steps(steps: IVec2) -> [Option<(MotorDir, u16)>; 2] {
    let x_args = match steps.x {
        0 => None,
        steps @ 1..=i32::MAX => Some((MotorDir::XPos, steps.abs() as u16)),
        steps @ i32::MIN..=-1 => Some((MotorDir::XNeg, steps.abs() as u16)),
    };
    let y_args = match steps.y {
        0 => None,
        steps @ 1..=i32::MAX => Some((MotorDir::YPos, steps.abs() as u16)),
        steps @ i32::MIN..=-1 => Some((MotorDir::YNeg, steps.abs() as u16)),
    };
    [x_args, y_args]
}
