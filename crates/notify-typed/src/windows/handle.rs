use std::{os::windows::ffi::OsStrExt, path::Path};

use windows::{
    self,
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
            FILE_NOTIFY_CHANGE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING, ReadDirectoryChangesExW, ReadDirectoryNotifyExtendedInformation,
        },
        System::IO::OVERLAPPED,
    },
    core::{Error, Free as _, PCWSTR},
};

pub struct DirHandle(pub(super) HANDLE);
impl DirHandle {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let wide_name = path
            .as_ref()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                PCWSTR::from_raw(wide_name.as_ptr()),
                FILE_LIST_DIRECTORY.0, // access type (`list_directory`` for dir watching)
                FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE, // (allow other programs access while handle open)
                None,          // don't need any security attributes
                OPEN_EXISTING, // open an existing file (directory)
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED, // `overlapped` for async use, `backup_semantics` for opening a dir
                None, // not opening a file, no template
            )
        }?;
        Ok(Self(handle))
    }
    pub fn read_dir_changes_ex_overlapped(
        &self,
        buffer: &mut [u8],
        recursive: bool,
        filter: Filter,
        overlapped: &mut Overlapped,
    ) -> windows::core::Result<()> {
        unsafe {
            ReadDirectoryChangesExW(
                self.0,
                buffer.as_mut_ptr().cast(),
                buffer
                    .len()
                    .try_into()
                    .expect("buffer len should always fit in u32"),
                recursive,
                FILE_NOTIFY_CHANGE(filter.bits()),
                None,
                Some(&mut overlapped.0),
                None,
                ReadDirectoryNotifyExtendedInformation,
            )
        }
    }
}
impl Drop for DirHandle {
    fn drop(&mut self) {
        unsafe { self.0.free() }
    }
}
unsafe impl Send for DirHandle {}
unsafe impl Sync for DirHandle {}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct Filter: u32{
        const FileCRD = 0x1;
        const DirCRD = 0x2;
        const Attrs = 0x4;
        const Size = 0x8;
        const LastWrite = 0x10;
        const LastAccess = 0x20;
        const CreationTime = 0x40;
        const Security = 0x100;
    }
}

pub struct Overlapped(OVERLAPPED);
impl Overlapped {
    pub fn new() -> Self {
        Self(OVERLAPPED::default())
    }
}
unsafe impl Send for Overlapped {}
unsafe impl Sync for Overlapped {}
