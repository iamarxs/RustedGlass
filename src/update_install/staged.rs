use std::fs::File;
use std::io::{self, Read};
use std::mem::{offset_of, size_of, size_of_val};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FileRenameInfo, SetFileInformationByHandle, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO, FILE_SHARE_READ,
};

use crate::update_package::MAX_EXE;

/// The verified file stays open without write or delete sharing until it is installed.
pub(super) struct VerifiedFile(File);

impl VerifiedFile {
    pub(super) fn open(path: &Path, expected: [u8; 32]) -> Result<Self, String> {
        let mut file = File::options()
            .access_mode(GENERIC_READ.0 | DELETE.0)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)
            .map_err(read_error)?;
        let metadata = file.metadata().map_err(read_error)?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err("The staged update must be a real executable file.".into());
        }
        if metadata.len() > MAX_EXE as u64 {
            return Err("The staged executable exceeds the update size limit.".into());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 8192];
        loop {
            let read = file.read(&mut buffer).map_err(read_error)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        if <[u8; 32]>::from(hash.finalize()) != expected {
            return Err("The staged update does not match the verified download. The current version is still installed.".into());
        }
        Ok(Self(file))
    }

    /// Rename the verified handle itself. The source name is never reopened for the swap.
    pub(super) fn replace(self, target: &Path) -> Result<(), String> {
        let name: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
        let offset = offset_of!(FILE_RENAME_INFO, FileName);
        let size = (offset + size_of_val(name.as_slice())).max(size_of::<FILE_RENAME_INFO>());
        // Allocate aligned storage for the Win32 header and its variable-length UTF-16 name.
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous.ReplaceIfExists = true;
            (*info).FileNameLength = ((name.len() - 1) * size_of::<u16>()) as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                storage.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
                name.len(),
            );
            SetFileInformationByHandle(
                HANDLE(self.0.as_raw_handle()),
                FileRenameInfo,
                storage.as_ptr().cast(),
                size as u32,
            )
        }
        .map_err(|error| error.to_string())
    }
}

fn read_error(error: io::Error) -> String {
    format!("Could not read the staged update: {error}")
}
