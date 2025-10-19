use std::ptr;

use windows::{
    Win32::System::Threading::{
        CancelThreadpoolIo, CreateThreadpoolIo, PTP_CALLBACK_INSTANCE, PTP_IO, StartThreadpoolIo,
    },
    core::{Error, Free as _},
};

use crate::windows::handle::DirHandle;

pub struct ThreadPoolIO<C>
where
    C: ThreadPoolCallback,
{
    pub(super) callback: Box<C>,
    ptp_io: PTP_IO,
}
impl<C> ThreadPoolIO<C>
where
    C: ThreadPoolCallback,
{
    pub fn new(handle: &DirHandle, callback: C) -> Result<Self, Error> {
        let mut callback = Box::new(callback);
        let callback_ptr = ptr::from_mut(callback.as_mut()).cast();
        let ptp_io = unsafe {
            CreateThreadpoolIo(handle.0, Some(callback_fn::<C>), Some(callback_ptr), None)
        }?;
        Ok(Self { callback, ptp_io })
    }
    pub fn start(&self) {
        unsafe { StartThreadpoolIo(self.ptp_io) };
    }
    pub fn cancel(&self) {
        unsafe {
            CancelThreadpoolIo(self.ptp_io);
        };
    }
}
impl<C> Drop for ThreadPoolIO<C>
where
    C: ThreadPoolCallback,
{
    fn drop(&mut self) {
        self.cancel();
        unsafe {
            self.ptp_io.free();
        };
    }
}
pub trait ThreadPoolCallback {
    fn call(&mut self, bytes_written: Result<usize, u32>);
}

unsafe extern "system" fn callback_fn<C: ThreadPoolCallback>(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut core::ffi::c_void,
    _overlapped: *mut core::ffi::c_void,
    io_result: u32,
    num_bytes: usize,
    io: PTP_IO,
) {
    unsafe {
        StartThreadpoolIo(io);
    };
    let num_bytes = match io_result {
        0 => Ok(num_bytes),
        n => Err(n),
    };
    let callback =
        unsafe { context.cast::<C>().as_mut() }.expect("context pointer should never be null");
    callback.call(num_bytes);
}
