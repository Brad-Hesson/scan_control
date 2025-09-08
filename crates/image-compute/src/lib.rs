#![allow(dead_code)]

mod buffers;
pub mod image_compute;
pub mod scan_image;
mod shaders;

#[cfg(test)]
mod tests {
    use eyre::{Context, Result};
    use itertools::izip;
    use tracing::info;
    use tracing_subscriber::EnvFilter;
    use wgpu::{
        Adapter, CommandEncoderDescriptor, ComputePassDescriptor, Device, DeviceDescriptor,
        FeaturesWGPU, FeaturesWebGPU, Instance, PollType, PowerPreference, Queue,
        RequestAdapterOptions,
    };

    use crate::image_compute::{ImageComputeBuffers, ImageComputePipeline};

    #[test]
    fn output() -> Result<()> {
        let (_instance, _adapter, device, queue) = init().context("Init failed")?;
        const WIDTH: usize = 512;
        const HEIGHT: usize = 1024;
        const SIZE: [u32; 2] = [WIDTH as _, HEIGHT as _];
        let mut plane_fitter = ImageComputePipeline::new(&device);
        let x_slope = 1.0;
        let y_slope = 10.0;
        let offset = 0.0;
        let mut mean = 0.;
        let init_data = |data: &mut [f32]| {
            // data.fill(1.);
            // for i in 0..data.len() {
            //     data[i] = (i / 32) as f32;
            // }
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let dat = &mut data[y * WIDTH + x];
                    let (x, y) = (x as f32 / WIDTH as f32, y as f32 / HEIGHT as f32);
                    *dat = (x_slope / WIDTH as f32) * x + (y_slope / HEIGHT as f32) * y;
                    let val = x_slope * x + y_slope * y + offset;
                    // *dat = y as f32;
                    // mean += val as f64;
                }
            }
        };
        let original = ImageComputeBuffers::new(
            &device,
            Some("original_image"),
            SIZE,
            HEIGHT as u32,
            init_data,
        );
        mean /= (WIDTH * HEIGHT) as f64;
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        unsafe { device.start_graphics_debugger_capture() };
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("test_name"),
        });
        let n_times;
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Test Compute Pass"),
                timestamp_writes: None,
            });
            n_times = plane_fitter.dispatch_mean_subtract(&device, &mut pass, &original);
        }
        plane_fitter.resolve_timings(&mut encoder, n_times);
        device.poll(PollType::WaitForSubmissionIndex(
            queue.submit([encoder.finish()]),
        ))?;
        unsafe { device.stop_graphics_debugger_capture() };
        let meta_download = original.download_planarize_data(&device, &queue, 16);
        let normal_download = original.download_normalize_data(&device, &queue);
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        println!(
            "0: {}, 1: {}, 2: {}",
            meta_download.get().unwrap()[0],
            meta_download.get().unwrap()[1],
            meta_download.get().unwrap()[2],
        );
        println!("Actual:");
        println!("a: {}, x: {}, y: {}", mean, x_slope, y_slope);
        Ok(())
    }
    #[test]
    fn timing() -> Result<()> {
        let (_instance, _adapter, device, queue) = init().context("Init failed")?;
        const WIDTH: usize = 1024;
        const HEIGHT: usize = 1024;
        const SIZE: [u32; 2] = [WIDTH as _, HEIGHT as _];
        let mut plane_fitter = ImageComputePipeline::new(&device);
        let original =
            ImageComputeBuffers::new(&device, Some("original_image"), SIZE, HEIGHT as u32, |_| {});
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        let mut times = vec![0., 0., 0., 0., 0.];
        let mut latest = vec![0., 0., 0., 0., 0.];
        let mult = queue.get_timestamp_period();
        for i in 1.. {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("test_name"),
            });
            let n_times;
            {
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("Test Compute Pass"),
                    timestamp_writes: None,
                });
                n_times = plane_fitter.dispatch_mean_subtract(&device, &mut pass, &original);
            }
            plane_fitter.resolve_timings(&mut encoder, n_times);
            device.poll(PollType::WaitForSubmissionIndex(
                queue.submit([encoder.finish()]),
            ))?;
            let times_download = plane_fitter.queue_timings_download(&device, &queue, n_times);
            device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
            let new_times = times_download
                .get()
                .unwrap()
                .iter()
                .map(|v| *v as f64 / 1000. * mult as f64);
            let x = 1. / (i as f64);
            for (mean, late, new) in izip!(times.iter_mut(), latest.iter_mut(), new_times) {
                *late = new;
                *mean = *mean * (1. - x) + new * x;
            }
            if i % 100 == 0 {
                println!(
                    "            {latest:9.4?} -> {:9.4} micros",
                    latest.iter().sum::<f64>()
                );
                println!(
                    "{x:11.6} {times:9.4?} -> {:9.4} micros",
                    times.iter().sum::<f64>()
                );
                println!();
            }
        }
        Ok(())
    }
    fn lerp(s: f64, e: f64, t: f64) -> f64 {
        t * e + (1. - t) * s
    }
    fn init() -> Result<(Instance, Adapter, Device, Queue)> {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        let instance = wgpu::Instance::default();
        let adapter = smol::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .context("Adapter request failed")?;
        info!("Backend: {}", adapter.get_info().backend.to_str());
        info!("Adapter: {}", adapter.get_info().name);
        info!(
            "Driver: {} {}",
            adapter.get_info().driver,
            adapter.get_info().driver_info
        );
        let (dev, queue) = smol::block_on(adapter.request_device(&DeviceDescriptor {
            required_features: wgpu::Features {
                features_wgpu: FeaturesWGPU::TIMESTAMP_QUERY_INSIDE_PASSES
                    | FeaturesWGPU::SHADER_F64
                    | FeaturesWGPU::SHADER_INT64,
                features_webgpu: FeaturesWebGPU::FLOAT32_FILTERABLE
                    | FeaturesWebGPU::TIMESTAMP_QUERY,
            },
            ..Default::default()
        }))
        .context("Device request failed")?;
        Ok((instance, adapter, dev, queue))
    }
}
