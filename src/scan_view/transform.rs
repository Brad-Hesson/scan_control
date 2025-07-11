#![allow(dead_code)]
use eframe::wgpu::{
    BufferUsages, CommandEncoder, ComputePass, ComputePassDescriptor, ComputePipeline, Device,
};

use super::shaders::{transform, StorageBuffer};

pub struct Transformer {
    add_pipeline: ComputePipeline,
    mul_pipeline: ComputePipeline,
    div_pipeline: ComputePipeline,
    copy_pipeline: ComputePipeline,
    col_broadcast_pipeline: ComputePipeline,
    row_broadcast_pipeline: ComputePipeline,
    col_sum_pipeline: ComputePipeline,
    row_sum_pipeline: ComputePipeline,
    iter1_bind_group: transform::IterationBindGroup,
    iter2_bind_group: transform::IterationBindGroup,
}
impl Transformer {
    pub fn new(device: &Device) -> Self {
        let iter1_buf = StorageBuffer::new_as(device, &[0], BufferUsages::UNIFORM);
        let iter2_buf = StorageBuffer::new_as(device, &[1], BufferUsages::UNIFORM);
        Self {
            col_sum_pipeline: transform::create_col_sum_pipeline(device),
            row_sum_pipeline: transform::create_row_sum_pipeline(device),
            add_pipeline: transform::create_add_pipeline(device),
            iter1_bind_group: transform::IterationBindGroup::new(device, &iter1_buf),
            iter2_bind_group: transform::IterationBindGroup::new(device, &iter2_buf),
            mul_pipeline: transform::create_mul_pipeline(device),
            div_pipeline: transform::create_div_pipeline(device),
            copy_pipeline: transform::create_copy_pipeline(device),
            col_broadcast_pipeline: transform::create_col_broadcast_pipeline(device),
            row_broadcast_pipeline: transform::create_row_broadcast_pipeline(device),
        }
    }
    pub fn col_sum(&self, pass: &mut ComputePass, data: &Transformable) {
        pass.set_pipeline(&self.col_sum_pipeline);
        data.bind_group.set(pass, 0);
        data.bind_group.set(pass, 1); // unused as the op is unary
        data.bind_group.set(pass, 2); // unused as the op is unary
        self.iter1_bind_group.set(pass);
        pass.dispatch_workgroups(data.width as u32, data.height as u32, 1);
        self.iter2_bind_group.set(pass);
        pass.dispatch_workgroups(
            data.width as u32,
            div_ceil(data.height as u32, transform::workgroup_size),
            1,
        );
    }
    pub fn row_sum(&self, pass: &mut ComputePass, data: &Transformable) {
        pass.set_pipeline(&self.row_sum_pipeline);
        data.bind_group.set(pass, 0);
        data.bind_group.set(pass, 1); // unused as the op is unary
        data.bind_group.set(pass, 2); // unused as the op is unary
        self.iter1_bind_group.set(pass);
        pass.dispatch_workgroups(data.width as u32, data.height as u32, 1);
        self.iter2_bind_group.set(pass);
        pass.dispatch_workgroups(
            div_ceil(data.width as u32, transform::workgroup_size),
            data.height as u32,
            1,
        );
    }
    pub fn sum(&self, pass: &mut ComputePass, data: &Transformable) {
        self.col_sum(pass, data);
        self.row_sum(pass, data);
    }
    pub fn add(
        &self,
        pass: &mut ComputePass,
        data_a: &Transformable,
        data_b: &Transformable,
        data_out: &Transformable,
    ) {
        self.simple_pipeline(pass, &self.add_pipeline, [data_out, data_a, data_b]);
    }
    pub fn mul(
        &self,
        pass: &mut ComputePass,
        data_a: &Transformable,
        data_b: &Transformable,
        data_out: &Transformable,
    ) {
        self.simple_pipeline(pass, &self.mul_pipeline, [data_out, data_a, data_b]);
    }
    pub fn div(
        &self,
        pass: &mut ComputePass,
        data_a: &Transformable,
        data_b: &Transformable,
        data_out: &Transformable,
    ) {
        self.simple_pipeline(pass, &self.div_pipeline, [data_out, data_a, data_b]);
    }
    pub fn copy(&self, pass: &mut ComputePass, data_in: &Transformable, data_out: &Transformable) {
        self.simple_pipeline(pass, &self.copy_pipeline, [data_out, data_in, data_in]);
    }
    pub fn row_broadcast(
        &self,
        pass: &mut ComputePass,
        data_in: &Transformable,
        data_out: &Transformable,
    ) {
        self.simple_pipeline(
            pass,
            &self.row_broadcast_pipeline,
            [data_out, data_in, data_in],
        );
    }
    pub fn col_broadcast(
        &self,
        pass: &mut ComputePass,
        data_in: &Transformable,
        data_out: &Transformable,
    ) {
        self.simple_pipeline(
            pass,
            &self.col_broadcast_pipeline,
            [data_out, data_in, data_in],
        );
    }
    fn simple_pipeline(
        &self,
        pass: &mut ComputePass,
        pipeline: &ComputePipeline,
        datas: [&Transformable; 3],
    ) {
        // for data in datas.iter().skip(1) {
        //     assert!(datas[0].same_size(data));
        //     assert!(!datas[0].same_buffer(data));
        // }
        pass.set_pipeline(pipeline);
        datas[0].bind_group.set(pass, 0);
        datas[1].bind_group.set(pass, 1);
        datas[2].bind_group.set(pass, 2);
        self.iter1_bind_group.set(pass); // unused, but necessary
        pass.dispatch_workgroups(datas[0].width as u32, datas[0].height as u32, 1);
    }
}

fn div_ceil(a: u32, b: u32) -> u32 {
    (a + b - 1) / b
}

pub struct Transformable {
    pub width: usize,
    pub height: usize,
    pub bind_group: transform::DataBindGroup,
    pub data_buf: StorageBuffer<f32>,
}
impl Transformable {
    pub fn new(device: &Device, width: usize, height: usize, data_buf: StorageBuffer<f32>) -> Self {
        let size_buf = StorageBuffer::new_as(
            &device,
            &[width as u32, height as u32],
            BufferUsages::UNIFORM,
        );
        let bind_group = transform::DataBindGroup::new(&device, &size_buf, &data_buf);
        Self {
            width,
            height,
            bind_group,
            data_buf,
        }
    }
    pub fn new_of_size(device: &Device, width: usize, height: usize) -> Self {
        let data_buf = StorageBuffer::new(
            device,
            width * height,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        Self::new(device, width, height, data_buf)
    }
    pub fn new_like(device: &Device, other: &Transformable) -> Self {
        Self::new_of_size(device, other.width, other.height)
    }
    pub fn same_size(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height
    }
    pub fn same_buffer(&self, other: &Self) -> bool {
        self.data_buf.global_id() == other.data_buf.global_id()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicBool, Arc};

    use eframe::wgpu::{
        self, CommandEncoderDescriptor, DeviceDescriptor, RequestAdapterOptionsBase,
    };

    use crate::scan_view::transform::{Transformable, Transformer};

    use super::*;

    #[test]
    fn sum_shader() {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptionsBase::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let trans = Transformer::new(&device);

        let width = 60;
        let height = 60;
        println!("should be {}", width * height);
        let buf_size = width * height;
        let a = Transformable::new(
            &device,
            width,
            height,
            StorageBuffer::new_with(
                &device,
                buf_size,
                1.,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            ),
        );
        let b = Transformable::new_like(&device, &a);
        let c = Transformable::new_like(&device, &a);

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
            // trans.copy(&mut pass, &a, &b);
            // trans.add(&mut pass, &a, &b, &c);
        }
        queue.submit([encoder.finish()]);

        let bar = Arc::new(AtomicBool::new(false));
        let bar_clone = bar.clone();
        wgpu::util::DownloadBuffer::read_buffer(
            &device,
            &queue,
            &a.data_buf.slice(..),
            move |res| {
                let raw = res.unwrap();
                let data = bytemuck::cast_slice::<_, f32>(&raw);
                for i in (0..height).rev() {
                    println!("{:?}", &data[i * width..][..width]);
                }
                println!("Done");
                bar_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            },
        );
        println!("before");
        while !device.poll(wgpu::MaintainBase::Wait).is_queue_empty() {}
        while !bar.load(std::sync::atomic::Ordering::SeqCst) {}
        println!("after");
    }
}
