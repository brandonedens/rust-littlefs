
#![no_std]

#[macro_use] extern crate bitflags;

const READ_SIZE: usize = 256;
const PROG_SIZE: usize = 512;
const BLOCK_SIZE: usize = 4096;
const BLOCK_COUNT: usize = 32;
const LOOKAHEAD: usize = 64;

use littlefs_sys as lfs;
use core::{cmp, mem, slice};

#[derive(Debug)]
pub enum FsError {
    Io,
    Corrupt,
    Noent,
    Exist,
    NotDir,
    IsDir,
    NotEmpty,
    Badf,
    FBig,
    Inval,
    Nospc,
    Nomem,
}

pub trait Storage {
    fn read(&self, off: usize, buf: &mut[u8]) -> Result<usize, FsError>;
    fn write(&mut self, off: usize, data: &[u8]) -> Result<usize, FsError>;
    fn erase(&mut self, off: usize, len: usize) -> Result<usize, FsError>;
}

/// Convert an lfs error to a FsError.
fn lfs_to_fserror(lfs_error: lfs::lfs_error) -> Result<(), FsError> {
    match lfs_error {
        lfs::lfs_error_LFS_ERR_IO => Err(FsError::Io),
        lfs::lfs_error_LFS_ERR_CORRUPT => Err(FsError::Corrupt),
        lfs::lfs_error_LFS_ERR_NOENT =>    Err(FsError::Noent),
        lfs::lfs_error_LFS_ERR_EXIST =>    Err(FsError::Exist),
        lfs::lfs_error_LFS_ERR_NOTDIR =>   Err(FsError::NotDir),
        lfs::lfs_error_LFS_ERR_ISDIR =>    Err(FsError::IsDir),
        lfs::lfs_error_LFS_ERR_NOTEMPTY => Err(FsError::NotEmpty),
        lfs::lfs_error_LFS_ERR_BADF     => Err(FsError::Badf),
        lfs::lfs_error_LFS_ERR_FBIG     => Err(FsError::FBig),
        lfs::lfs_error_LFS_ERR_INVAL    => Err(FsError::Inval),
        lfs::lfs_error_LFS_ERR_NOSPC    => Err(FsError::Nospc),
        lfs::lfs_error_LFS_ERR_NOMEM    => Err(FsError::Nomem),
        _ => Ok(())
    }
}

enum Whence {
    Set = 0,
    Cu = 1,
    End = 2,
}

bitflags! {
    struct FileOpenFlags: u32 {
        const RDONLY = 0x1;
        const WRONLY = 0x2;
        const RDWR = Self::RDONLY.bits | Self::WRONLY.bits;
        const CREAT = 0x0100;
        const EXCL = 0x0200;
        const TRUNC = 0x0400;
        const APPEND = 0x0800;
    }
}

struct File {
    inner: lfs::lfs_file_t,
}

impl Default for File {
    fn default() -> Self {
        File { inner: unsafe { mem::uninitialized() } }
    }
}

struct LittleFs<T: Storage> {
    storage: T,
    lfs_config: lfs::lfs_config,
    lfs: lfs::lfs_t,
    read_buffer: [u8; READ_SIZE],
    prog_buffer: [u8; PROG_SIZE],
    lookahead_buffer: [u8; LOOKAHEAD / 8],
}

            // self.lfs_config.context: self as *mut _ as *mut libc::c_void,
impl<T: Storage> LittleFs<T> {
    pub fn new(storage: T) -> Self {
        LittleFs {
            storage: storage,
            lfs: unsafe { mem::uninitialized::<lfs::lfs>() },
            lfs_config: unsafe { mem::uninitialized::<lfs::lfs_config>() },
            read_buffer: [0u8; READ_SIZE],
            prog_buffer: [0u8; PROG_SIZE],
            lookahead_buffer: [0u8; LOOKAHEAD / 8],
        }
    }

    /// Format the filesystem.
    pub fn format(&mut self) -> Result<(), FsError> {
        self.lfs_config = self.create_lfs_config();
        let res = unsafe { lfs::lfs_format(&mut self.lfs, &self.lfs_config) };
        lfs_to_fserror(res)
    }

    /// Mount the filesystem.
    pub fn mount(&mut self) -> Result<(), FsError> {
        self.lfs_config = self.create_lfs_config();
        let res = unsafe { lfs::lfs_mount(&mut self.lfs, &self.lfs_config) };
        lfs_to_fserror(res)
    }

    /// Unmount the filesystem.
    pub fn unmount(mut self) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_unmount(&mut self.lfs) };
        lfs_to_fserror(res)
    }

    /// Remove a file or directory.
    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let mut cstr = [0u8; lfs::LFS_NAME_MAX as usize];
        cstr.copy_from_slice(&path.as_bytes()[..]);
        let res = unsafe { lfs::lfs_remove(&mut self.lfs, &cstr as *const _ as *const libc::c_char) };
        lfs_to_fserror(res)
    }

    /// Rename of move a file or directory.
    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        let mut oldpath = [0u8; lfs::LFS_NAME_MAX as usize];
        let oldpathlen = cmp::min(lfs::LFS_NAME_MAX as usize - 1, old_path.len());
        let mut newpath = [0u8; lfs::LFS_NAME_MAX as usize];
        let newpathlen = cmp::min(lfs::LFS_NAME_MAX as usize - 1, new_path.len());
        oldpath[..oldpathlen].copy_from_slice(&old_path.as_bytes()[..oldpathlen]);
        newpath[..newpathlen].copy_from_slice(&new_path.as_bytes()[..newpathlen]);
        let res = unsafe {
            lfs::lfs_rename(&mut self.lfs,
                            &oldpath as *const _ as *const libc::c_char,
                            &newpath as *const _ as *const libc::c_char)
        };
        lfs_to_fserror(res)
    }

    /// Open a file at the given path.
    pub fn file_open(&mut self, file: &mut File,
                     path: &str, flags: FileOpenFlags) -> Result<(), FsError> {
        let mut cstr_path = [0u8; lfs::LFS_NAME_MAX as usize];
        let len = cmp::min(lfs::LFS_NAME_MAX as usize - 1, path.len());
        cstr_path[..len].copy_from_slice(&path.as_bytes()[..len]);
        let res = unsafe {
            lfs::lfs_file_open(&mut self.lfs,
                               &mut file.inner,
                               &cstr_path as *const _ as *const libc::c_char,
                               flags.bits() as i32)
        };
        lfs_to_fserror(res)
    }

    // TODO file_opencfg.
    
    /// Close out the given file.
    pub fn file_close(&mut self, mut file: File) -> Result<(), FsError> {
        let res = unsafe {
            lfs::lfs_file_close(&mut self.lfs,
                               &mut file.inner)
        };
        lfs_to_fserror(res)
    }

    // TODO Synchronize file state to storage.
    
    /// Read data from file.
    pub fn file_read(&mut self, file: &mut File, buf: &mut [u8]) -> Result<(), FsError> {
        let res = unsafe {
            lfs::lfs_file_read(&mut self.lfs,
                               &mut file.inner,
                               buf.as_mut_ptr() as *mut libc::c_void,
                               buf.len() as u32)
        };
        lfs_to_fserror(res)
    }

    pub fn file_write(&mut self, file: &mut File, buf: &[u8]) -> Result<(), FsError> {
        let res = unsafe {
            lfs::lfs_file_read(&mut self.lfs,
                               &mut file.inner,
                               buf.as_ptr() as *mut libc::c_void,
                               buf.len() as u32)
        };
        lfs_to_fserror(res)
    }

    pub fn file_seek(&mut self, file: &mut File, off: usize, whence: Whence) -> Result<(), FsError> {
        Ok(())
    }

    pub fn file_truncate(&mut self, file: &mut File, size: usize) -> Result<(), FsError> {
        Ok(())
    }



    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let mut cstr_path = [0u8; lfs::LFS_NAME_MAX as usize];
        let len = cmp::min(lfs::LFS_NAME_MAX as usize - 1, path.len());
        cstr_path[..len].copy_from_slice(&path.as_bytes()[..len]);

        let res = unsafe {
            lfs::lfs_mkdir(&mut self.lfs,
                               &cstr_path as *const _ as *const libc::c_char)
        };
        lfs_to_fserror(res)
    }


    /// Create instance of lfs configuration.
    fn create_lfs_config(&mut self) -> lfs::lfs_config {
        lfs::lfs_config {
            context: self as *mut _ as *mut libc::c_void,
            read: Some(<LittleFs<T>>::lfs_config_read),
            prog: Some(<LittleFs<T>>::lfs_config_prog),
            erase: Some(<LittleFs<T>>::lfs_config_erase),
            sync: Some(<LittleFs<T>>::lfs_config_sync),
            read_size: READ_SIZE as u32,
            prog_size: PROG_SIZE as u32,
            block_size: BLOCK_SIZE as u32,
            block_count: BLOCK_COUNT as u32,
            lookahead: LOOKAHEAD as u32,
            read_buffer: (&mut self.read_buffer) as *mut _ as *mut libc::c_void,
            prog_buffer: (&mut self.prog_buffer) as *mut _ as *mut libc::c_void,
            lookahead_buffer: (&mut self.lookahead_buffer) as *mut _ as *mut libc::c_void,
            file_buffer: core::ptr::null_mut(),
        }
    }

    extern "C" fn lfs_config_read(c: *const lfs::lfs_config,
                block: lfs::lfs_block_t,
                off: lfs::lfs_off_t,
                buffer: *mut libc::c_void,
                size: lfs::lfs_size_t,
                ) -> libc::c_int {
        let littlefs: &mut LittleFs<T> =
            unsafe { mem::transmute((*c).context) };
        let off = off as usize;
        let buf: &mut [u8] =
            unsafe {
                slice::from_raw_parts_mut(buffer as *mut u8, size as usize)
            };

        // TODO
        littlefs.storage.read(off, buf).unwrap();
        0
    }

    extern "C" fn lfs_config_prog(c: *const lfs::lfs_config,
                block: lfs::lfs_block_t,
                off: lfs::lfs_off_t,
                buffer: *const libc::c_void,
                size: lfs::lfs_size_t,
                ) -> libc::c_int {
        let littlefs: &mut LittleFs<T> = unsafe { mem::transmute((*c).context) };
        let off = off as usize;
        let buf: &[u8] =
            unsafe {
                slice::from_raw_parts(buffer as *const u8, size as usize)
            };

        // TODO
        littlefs.storage.write(off, buf).unwrap();
        0
    }

    extern "C" fn lfs_config_erase(c: *const lfs::lfs_config,
                block: lfs::lfs_block_t) -> libc::c_int {
        let littlefs: &mut LittleFs<T> = unsafe { mem::transmute((*c).context) };
        let off = block as usize * BLOCK_SIZE;

        // TODO
        littlefs.storage.erase(off, BLOCK_SIZE).unwrap();
        0
    }

    extern "C" fn lfs_config_sync(c: *const lfs::lfs_config) -> i32 {
        // Do nothing; we presume that data is synchronized.
        0
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Default flash erase value.
    const ERASE_VALUE: u8 = 0xFF;

    const STORAGE_SIZE: usize = 131072;
    struct RamStorage {
        buf: [u8; STORAGE_SIZE],
    }

    impl Default for RamStorage {
        fn default() -> Self {
            RamStorage { buf: [ERASE_VALUE; STORAGE_SIZE] }
        }
    }

    impl Storage for RamStorage {
        fn read(&self, off: usize, buf: &mut[u8]) -> Result<usize, FsError> {
            for i in 0..buf.len() {
                if off + i >= self.buf.len() {
                    break;
                }
                buf[i] = self.buf[off + i];
            }
            Ok(buf.len())
        }

        fn write(&mut self, off: usize, data: &[u8]) -> Result<usize, FsError> {
            for i in 0..data.len() {
                if off + i >= self.buf.len() {
                    break;
                }
                self.buf[off + i] = data[i];
            }
            Ok(data.len())
        }

        fn erase(&mut self, off: usize, len: usize) -> Result<usize, FsError> {
            for byte in &mut self.buf[off..off+len] {
                *byte = ERASE_VALUE;
            }
            Ok(len)
        }
    }

    #[test]
    fn test_create_littlefs() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
    }

    #[test]
    fn test_mount_littlefs() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        lfs.unmount().unwrap();
    }

    #[test]
    fn test_mkdir() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        lfs.mkdir("/foo").unwrap();
        lfs.unmount().unwrap();
    }

    /*
    #[test]
    fn test_create_file() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(&mut file, "/foo.txt", FileOpenFlags::RDWR | FileOpenFlags::CREAT).unwrap();
        lfs.unmount().unwrap();
    }
    */
}
