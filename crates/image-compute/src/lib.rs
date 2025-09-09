#![allow(dead_code)]

mod buffers;
pub mod image_compute;
pub mod scan_image;
mod shaders;

#[cfg(test)]
mod tests {
    use eyre::{Context, Result};
    use itertools::{Itertools, izip};
    use tracing::info;
    use tracing_subscriber::EnvFilter;
    use wgpu::{
        Adapter, CommandEncoderDescriptor, ComputePass, ComputePassDescriptor, Device,
        DeviceDescriptor, FeaturesWGPU, FeaturesWebGPU, Instance, PollType, PowerPreference, Queue,
        RequestAdapterOptions,
    };

    use crate::image_compute::{ImageComputeBuffers, ImageComputePipeline};

    #[test]
    fn output() -> Result<()> {
        let (_instance, _adapter, device, queue) = init().context("Init failed")?;
        const WIDTH: usize = 512;
        const HEIGHT: usize = 5;
        const SIZE: [u32; 2] = [WIDTH as _, HEIGHT as _];
        let mut plane_fitter = ImageComputePipeline::new(&device);
        let x_slope = 1.0;
        let y_slope = 10.0;
        let offset = 0.0;
        let mut mean = 0.;
        let init_data = |data: &mut [f32]| {
            // data.fill(1.);
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let dat = &mut data[y * WIDTH + x];
                    *dat = y as f32;
                }
            }
        };
        let original = ImageComputeBuffers::new(
            &device,
            &queue,
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
            n_times = plane_fitter.dispatch_line_fit_subtract(&device, &mut pass, &original);
        }
        plane_fitter.resolve_timings(&mut encoder, n_times);
        device.poll(PollType::WaitForSubmissionIndex(
            queue.submit([encoder.finish()]),
        ))?;
        unsafe { device.stop_graphics_debugger_capture() };
        let planarize_download = original.download_planarize_data(&device, &queue, HEIGHT * 2);
        let normalize_download = original.download_normalize_data(&device, &queue);
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        let planarize = planarize_download.get().unwrap();
        let means: [f64; HEIGHT] = planarize[..HEIGHT]
            .iter()
            .map(|v| v / WIDTH as f64)
            .collect_array()
            .unwrap();
        let normalize = normalize_download.get().unwrap();
        println!("{:.4?}", means);
        println!("{:.4?}", &planarize[HEIGHT..]);
        println!("{normalize:?}");
        Ok(())
    }
    #[test]
    fn mean_timing() -> Result<()> {
        test_timing(|plane_fitter, device, pass, original| {
            plane_fitter.dispatch_mean_subtract(device, pass, original)
        })
    }
    #[test]
    fn line_mean_timing() -> Result<()> {
        test_timing(|plane_fitter, device, pass, original| {
            plane_fitter.dispatch_line_mean_subtract(device, pass, original)
        })
    }
    #[test]
    fn line_fit_timing() -> Result<()> {
        test_timing(|plane_fitter, device, pass, original| {
            plane_fitter.dispatch_line_fit_subtract(device, pass, original)
        })
    }
    #[test]
    fn plane_fit_timing() -> Result<()> {
        test_timing(|plane_fitter, device, pass, original| {
            plane_fitter.dispatch_plane_fit_subtract(device, pass, original)
        })
    }
    fn test_timing(
        f: impl Fn(&mut ImageComputePipeline, &Device, &mut ComputePass, &ImageComputeBuffers) -> usize,
    ) -> Result<()> {
        let (_instance, _adapter, device, queue) = init().context("Init failed")?;
        const WIDTH: usize = 1024;
        const HEIGHT: usize = 1024;
        const SIZE: [u32; 2] = [WIDTH as _, HEIGHT as _];
        let mut plane_fitter = ImageComputePipeline::new(&device);
        let original = ImageComputeBuffers::new(
            &device,
            &queue,
            Some("original_image"),
            SIZE,
            HEIGHT as u32,
            |_| {},
        );
        device.poll(PollType::WaitForSubmissionIndex(queue.submit([])))?;
        let mut times = vec![];
        let mut latest = vec![];
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
                n_times = f(&mut plane_fitter, &device, &mut pass, &original);
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
            if times.is_empty() {
                times = vec![0.; n_times];
                latest = vec![0.; n_times];
            }
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
