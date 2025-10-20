use std::{pin::Pin, ptr};

use windows::{
    Win32::System::Threading::{
        CancelThreadpoolIo, CreateThreadpoolIo, PTP_CALLBACK_INSTANCE, PTP_IO, StartThreadpoolIo,
    },
    core::{Error, Free as _},
};

use crate::windows::bindings::handle::DirHandle;

pub struct ThreadPoolIO<C: ThreadPoolCallback> {
    pub callback: Pin<Box<C>>,
    ptp_io: PTP_IO,
}
impl<C: ThreadPoolCallback> ThreadPoolIO<C> {
    pub fn new(handle: &DirHandle, mut callback: Pin<Box<C>>) -> Result<Self, Error> {
        // Safety: when the callback_fn dereferences this ptr, we immediately wrap it as pinned again
        let callback_ptr = ptr::from_mut(unsafe { callback.as_mut().get_unchecked_mut() }).cast();
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
impl<C: ThreadPoolCallback> Drop for ThreadPoolIO<C> {
    fn drop(&mut self) {
        self.cancel();
        unsafe {
            self.ptp_io.free();
        };
    }
}
pub trait ThreadPoolCallback {
    fn call(self: Pin<&mut Self>, bytes_written: Result<&BytesWritten, u32>);
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
        0 => Ok(&BytesWritten(num_bytes)),
        n => Err(n),
    };
    // Safety: We know that the context ptr points to a pinned instance of ThreadPoolCallback
    let callback = unsafe {
        Pin::new_unchecked(
            context
                .cast::<C>()
                .as_mut()
                .expect("context pointer should never be null"),
        )
    };
    callback.call(num_bytes);
}

pub struct BytesWritten(pub(super) usize);
