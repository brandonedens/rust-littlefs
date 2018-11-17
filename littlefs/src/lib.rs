#![allow(dead_code)]
#![allow(unused_variables)]
#![no_std]

#[macro_use]
extern crate bitflags;

const READ_SIZE: usize = 256;
const PROG_SIZE: usize = 256;
const BLOCK_SIZE: usize = 4096;
const BLOCK_COUNT: usize = 32;
const LOOKAHEAD: usize = 128;

use core::{cmp, mem, ptr, slice};
use littlefs_sys as lfs;

const NAME_MAX_LEN: usize = lfs::LFS_NAME_MAX as usize;

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
    fn read(&self, off: usize, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write(&mut self, off: usize, data: &[u8]) -> Result<usize, FsError>;
    fn erase(&mut self, off: usize, len: usize) -> Result<usize, FsError>;
}

/// Convert an lfs error to a FsError.
fn lfs_to_fserror(lfs_error: lfs::lfs_error) -> Result<(), FsError> {
    match lfs_error {
        lfs::lfs_error_LFS_ERR_IO => Err(FsError::Io),
        lfs::lfs_error_LFS_ERR_CORRUPT => Err(FsError::Corrupt),
        lfs::lfs_error_LFS_ERR_NOENT => Err(FsError::Noent),
        lfs::lfs_error_LFS_ERR_EXIST => Err(FsError::Exist),
        lfs::lfs_error_LFS_ERR_NOTDIR => Err(FsError::NotDir),
        lfs::lfs_error_LFS_ERR_ISDIR => Err(FsError::IsDir),
        lfs::lfs_error_LFS_ERR_NOTEMPTY => Err(FsError::NotEmpty),
        lfs::lfs_error_LFS_ERR_BADF => Err(FsError::Badf),
        lfs::lfs_error_LFS_ERR_FBIG => Err(FsError::FBig),
        lfs::lfs_error_LFS_ERR_INVAL => Err(FsError::Inval),
        lfs::lfs_error_LFS_ERR_NOSPC => Err(FsError::Nospc),
        lfs::lfs_error_LFS_ERR_NOMEM => Err(FsError::Nomem),
        _ => Ok(()),
    }
}

/// Convert an lfs error to a FsError.
fn lfs_to_usize_fserror(lfs_error: lfs::lfs_error) -> Result<usize, FsError> {
    match lfs_error {
        lfs::lfs_error_LFS_ERR_IO => Err(FsError::Io),
        lfs::lfs_error_LFS_ERR_CORRUPT => Err(FsError::Corrupt),
        lfs::lfs_error_LFS_ERR_NOENT => Err(FsError::Noent),
        lfs::lfs_error_LFS_ERR_EXIST => Err(FsError::Exist),
        lfs::lfs_error_LFS_ERR_NOTDIR => Err(FsError::NotDir),
        lfs::lfs_error_LFS_ERR_ISDIR => Err(FsError::IsDir),
        lfs::lfs_error_LFS_ERR_NOTEMPTY => Err(FsError::NotEmpty),
        lfs::lfs_error_LFS_ERR_BADF => Err(FsError::Badf),
        lfs::lfs_error_LFS_ERR_FBIG => Err(FsError::FBig),
        lfs::lfs_error_LFS_ERR_INVAL => Err(FsError::Inval),
        lfs::lfs_error_LFS_ERR_NOSPC => Err(FsError::Nospc),
        lfs::lfs_error_LFS_ERR_NOMEM => Err(FsError::Nomem),
        val => Ok(val as usize),
    }
}

enum Whence {
    Set = 0,
    Cur = 1,
    End = 2,
}

#[derive(Debug, PartialEq)]
enum EntryType {
    RegularFile,
    Directory,
}

struct Info {
    entry_type: EntryType,
    size: usize,
    name: [char; NAME_MAX_LEN],
}

fn strlen(txt: *const cty::c_char) -> usize {

    if txt == ptr::null() {
        return 0;
    }

    let mut i = 0;
    let mut t = txt;
    loop {
        let v = unsafe { t.read() };
        if v == ('\0' as cty::c_char) {
            break;
        }
        t = unsafe { t.offset(1) };
        i += 1;
    }
    return i;
}

// FIXME
impl From<lfs::lfs_info> for Info {
    fn from(lfs_info: lfs::lfs_info) -> Info {
        let entry_type = match lfs_info.type_ as u32 {
            lfs::lfs_type_LFS_TYPE_REG => EntryType::RegularFile,
            lfs::lfs_type_LFS_TYPE_DIR => EntryType::Directory,
            _ => {
                unreachable!();
            }
        };

        let len = strlen(lfs_info.name.as_ptr());
        let name = unsafe { slice::from_raw_parts(lfs_info.name.as_ptr() as *const char, len) };

        let mut info = Info {
            entry_type: entry_type,
            size: lfs_info.size as usize,
            name: ['\0'; NAME_MAX_LEN],
        };
        info.name[..len].copy_from_slice(&name[..len]);
        info
    }
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
    buffer: [u8; PROG_SIZE],
    inner: lfs::lfs_file_t,
}

struct Dir {
    inner: lfs::lfs_dir_t,
}

impl Default for File {
    fn default() -> Self {
        File {
            buffer: [0u8; PROG_SIZE],
            inner: unsafe { mem::uninitialized() },
        }
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

// self.lfs_config.context: self as *mut _ as *mut cty::c_void,
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
        let mut cstr = [0u8; NAME_MAX_LEN + 1];
        let len = cmp::min(NAME_MAX_LEN, path.len());
        cstr[..len].copy_from_slice(&path.as_bytes()[..len]);
        let res =
            unsafe { lfs::lfs_remove(&mut self.lfs, &cstr as *const _ as *const cty::c_char) };
        lfs_to_fserror(res)
    }

    /// Rename or move a file or directory.
    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        let mut oldpath = [0u8; NAME_MAX_LEN + 1];
        let oldpathlen = cmp::min(NAME_MAX_LEN, old_path.len());
        let mut newpath = [0u8; NAME_MAX_LEN + 1];
        let newpathlen = cmp::min(NAME_MAX_LEN, new_path.len());
        oldpath[..oldpathlen].copy_from_slice(&old_path.as_bytes()[..oldpathlen]);
        newpath[..newpathlen].copy_from_slice(&new_path.as_bytes()[..newpathlen]);
        let res = unsafe {
            lfs::lfs_rename(
                &mut self.lfs,
                oldpath.as_ptr() as *const cty::c_char,
                newpath.as_ptr() as *const cty::c_char,
            )
        };
        lfs_to_fserror(res)
    }

    /// Populate info for file or directory at specified path.
    pub fn stat(&mut self, path: &str, info: &mut Info) -> Result<(), FsError> {
        let mut lfs_info: lfs::lfs_info = unsafe { mem::uninitialized() };

        let mut cstr = [0u8; NAME_MAX_LEN + 1];
        let len = cmp::min(NAME_MAX_LEN, path.len());
        cstr[..len].copy_from_slice(&path.as_bytes()[..len]);

        let res = unsafe {
            lfs::lfs_stat(
                &mut self.lfs,
                cstr.as_ptr() as *const cty::c_char,
                &mut lfs_info as *mut _,
            )
        };

        *info = lfs_info.into();
        lfs_to_fserror(res)
    }

    /// Open a file at the given path.
    pub fn file_open(
        &mut self,
        file: &mut File,
        path: &str,
        flags: FileOpenFlags,
    ) -> Result<(), FsError> {
        let mut cstr_path = [0u8; NAME_MAX_LEN];
        let len = cmp::min(NAME_MAX_LEN - 1, path.len());
        cstr_path[..len].copy_from_slice(&path.as_bytes()[..len]);
        let file_cfg = lfs::lfs_file_config {
            buffer: file.buffer.as_mut_ptr() as *mut cty::c_void,
        };
        let res = unsafe {
            lfs::lfs_file_opencfg(
                &mut self.lfs,
                &mut file.inner,
                cstr_path.as_ptr() as *const cty::c_char,
                flags.bits() as i32,
                &file_cfg,
            )
        };
        lfs_to_fserror(res)
    }

    /// Close out the given file.
    pub fn file_close(&mut self, mut file: File) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_file_close(&mut self.lfs, &mut file.inner) };
        lfs_to_fserror(res)
    }

    /// Synchronize file contents to storage.
    pub fn file_sync(&mut self, mut file: File) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_file_sync(&mut self.lfs, &mut file.inner) };
        lfs_to_fserror(res)
    }

    /// Read data from file.
    pub fn file_read(&mut self, file: &mut File, buf: &mut [u8]) -> Result<usize, FsError> {
        let res = unsafe {
            lfs::lfs_file_read(
                &mut self.lfs,
                &mut file.inner,
                buf.as_mut_ptr() as *mut cty::c_void,
                buf.len() as u32,
            )
        };
        lfs_to_usize_fserror(res)
    }

    /// Write data to file.
    pub fn file_write(&mut self, file: &mut File, buf: &[u8]) -> Result<usize, FsError> {
        let res = unsafe {
            lfs::lfs_file_write(
                &mut self.lfs,
                &mut file.inner,
                buf.as_ptr() as *const cty::c_void,
                buf.len() as u32,
            )
        };
        lfs_to_usize_fserror(res)
    }

    /// Change position of subsequent read / write in file.
    pub fn file_seek(
        &mut self,
        file: &mut File,
        off: isize,
        whence: Whence,
    ) -> Result<(), FsError> {
        let res = unsafe {
            lfs::lfs_file_seek(&mut self.lfs, &mut file.inner, off as i32, whence as i32)
        };
        lfs_to_fserror(res)
    }

    pub fn file_truncate(&mut self, file: &mut File, size: usize) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_file_truncate(&mut self.lfs, &mut file.inner, size as u32) };
        lfs_to_fserror(res)
    }

    /// Tell current position of handle within the file.
    pub fn file_tell(&mut self, file: &mut File) -> Result<usize, FsError> {
        let res = unsafe { lfs::lfs_file_tell(&mut self.lfs, &mut file.inner) };
        lfs_to_usize_fserror(res)
    }

    /// Rewind file handle to the start of the file.
    pub fn file_rewind(&mut self, file: &mut File) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_file_rewind(&mut self.lfs, &mut file.inner) };
        lfs_to_fserror(res)
    }

    /// Return total number of bytes in file.
    pub fn file_size(&mut self, file: &mut File) -> Result<usize, FsError> {
        let res = unsafe { lfs::lfs_file_size(&mut self.lfs, &mut file.inner) };
        lfs_to_usize_fserror(res)
    }

    /// Create a new directory.
    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let mut cstr_path = [0u8; NAME_MAX_LEN + 1];
        let len = cmp::min(NAME_MAX_LEN, path.len());
        cstr_path[..len].copy_from_slice(&path.as_bytes()[..len]);

        let res =
            unsafe { lfs::lfs_mkdir(&mut self.lfs, cstr_path.as_ptr() as *const cty::c_char) };
        lfs_to_fserror(res)
    }

    /// Open a directory.
    pub fn dir_open(&mut self, dir: &mut Dir, path: &str) -> Result<(), FsError> {
        let mut cstr_path = [0u8; NAME_MAX_LEN + 1];
        let len = cmp::min(NAME_MAX_LEN, path.len());
        cstr_path[..len].copy_from_slice(&path.as_bytes()[..len]);

        let res = unsafe {
            lfs::lfs_dir_open(
                &mut self.lfs,
                &mut dir.inner,
                cstr_path.as_ptr() as *const cty::c_char,
            )
        };
        lfs_to_fserror(res)
    }

    /// Close a directory.
    pub fn dir_close(&mut self, dir: &mut Dir) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_dir_close(&mut self.lfs, &mut dir.inner) };
        lfs_to_fserror(res)
    }

    /// Read contents of a directory.
    pub fn dir_read(&mut self, dir: &mut Dir, info: &mut Info) -> Result<(), FsError> {
        let mut lfs_info: lfs::lfs_info = unsafe { mem::uninitialized() };
        let res = unsafe { lfs::lfs_dir_read(&mut self.lfs, &mut dir.inner, &mut lfs_info) };
        *info = lfs_info.into();
        lfs_to_fserror(res)
    }

    /// Change the position within the directory.
    pub fn dir_seek(&mut self, dir: &mut Dir, offset: isize) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_dir_seek(&mut self.lfs, &mut dir.inner, offset as u32) };
        lfs_to_fserror(res)
    }

    /// Report position within the directory.
    pub fn dir_tell(&mut self, dir: &mut Dir) -> Result<usize, FsError> {
        let res = unsafe { lfs::lfs_dir_tell(&mut self.lfs, &mut dir.inner) };
        lfs_to_usize_fserror(res)
    }

    /// Rewrite directory handle back to start of directory.
    pub fn dir_rewind(&mut self, dir: &mut Dir) -> Result<(), FsError> {
        let res = unsafe { lfs::lfs_dir_rewind(&mut self.lfs, &mut dir.inner) };
        lfs_to_fserror(res)
    }

    /// Create instance of lfs configuration.
    fn create_lfs_config(&mut self) -> lfs::lfs_config {
        lfs::lfs_config {
            context: self as *mut _ as *mut cty::c_void,
            read: Some(<LittleFs<T>>::lfs_config_read),
            prog: Some(<LittleFs<T>>::lfs_config_prog),
            erase: Some(<LittleFs<T>>::lfs_config_erase),
            sync: Some(<LittleFs<T>>::lfs_config_sync),
            read_size: READ_SIZE as u32,
            prog_size: PROG_SIZE as u32,
            block_size: BLOCK_SIZE as u32,
            block_count: BLOCK_COUNT as u32,
            lookahead: LOOKAHEAD as u32,
            read_buffer: (&mut self.read_buffer) as *mut _ as *mut cty::c_void,
            prog_buffer: (&mut self.prog_buffer) as *mut _ as *mut cty::c_void,
            lookahead_buffer: (&mut self.lookahead_buffer) as *mut _ as *mut cty::c_void,
            file_buffer: core::ptr::null_mut(),
        }
    }

    extern "C" fn lfs_config_read(
        c: *const lfs::lfs_config,
        block: lfs::lfs_block_t,
        off: lfs::lfs_off_t,
        buffer: *mut cty::c_void,
        size: lfs::lfs_size_t,
    ) -> cty::c_int {
        let littlefs: &mut LittleFs<T> = unsafe { mem::transmute((*c).context) };
        assert!(!c.is_null());
        let block_size = unsafe { c.read().block_size };
        let off = (block * block_size + off) as usize;
        let buf: &mut [u8] = unsafe { slice::from_raw_parts_mut(buffer as *mut u8, size as usize) };

        // TODO
        littlefs.storage.read(off, buf).unwrap();
        0
    }

    extern "C" fn lfs_config_prog(
        c: *const lfs::lfs_config,
        block: lfs::lfs_block_t,
        off: lfs::lfs_off_t,
        buffer: *const cty::c_void,
        size: lfs::lfs_size_t,
    ) -> cty::c_int {
        let littlefs: &mut LittleFs<T> = unsafe { mem::transmute((*c).context) };
        assert!(!c.is_null());
        let block_size = unsafe { c.read().block_size };
        let off = (block * block_size + off) as usize;
        let buf: &[u8] = unsafe { slice::from_raw_parts(buffer as *const u8, size as usize) };

        // TODO
        littlefs.storage.write(off, buf).unwrap();
        0
    }

    extern "C" fn lfs_config_erase(
        c: *const lfs::lfs_config,
        block: lfs::lfs_block_t,
    ) -> cty::c_int {
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
            RamStorage {
                buf: [ERASE_VALUE; STORAGE_SIZE],
            }
        }
    }

    impl Storage for RamStorage {
        fn read(&self, off: usize, buf: &mut [u8]) -> Result<usize, FsError> {
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
            for byte in &mut self.buf[off..off + len] {
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

    #[test]
    fn test_create_file() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(
            &mut file,
            "/foo.txt",
            FileOpenFlags::RDWR | FileOpenFlags::CREAT,
        )
        .unwrap();
        lfs.unmount().unwrap();
    }

    #[test]
    fn test_write_file() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(
            &mut file,
            "/foo.txt",
            FileOpenFlags::RDWR | FileOpenFlags::CREAT,
        )
        .unwrap();
        let sz = lfs.file_write(&mut file, b"hello world!").unwrap();
        assert_ne!(sz, 0);
        lfs.file_close(file).unwrap();
        lfs.unmount().unwrap();
    }

    #[test]
    fn test_read_write_file() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(
            &mut file,
            "/foo.txt",
            FileOpenFlags::RDWR | FileOpenFlags::CREAT,
        )
        .unwrap();
        let write_sz = lfs.file_write(&mut file, b"hello world!").unwrap();
        assert_ne!(write_sz, 0);

        let file_sz = lfs.file_size(&mut file).unwrap();
        assert_eq!(file_sz, write_sz);

        lfs.file_close(file).unwrap();

        let mut file = Default::default();
        lfs.file_open(&mut file, "/foo.txt", FileOpenFlags::RDWR)
            .unwrap();
        let mut buf = [0u8; 32];
        let read_sz = lfs.file_read(&mut file, &mut buf).unwrap();
        assert_ne!(read_sz, 0);
        lfs.file_close(file).unwrap();
        lfs.unmount().unwrap();

        assert_eq!(read_sz, write_sz);

        assert_eq!(&buf[..read_sz], b"hello world!");
    }

    #[test]
    fn test_lfs_seek() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(
            &mut file,
            "/foo.txt",
            FileOpenFlags::RDWR | FileOpenFlags::CREAT,
        )
        .unwrap();
        let write_sz = lfs.file_write(&mut file, b"hello world!").unwrap();
        assert_ne!(write_sz, 0);
        lfs.file_close(file).unwrap();

        let mut file = Default::default();
        lfs.file_open(&mut file, "/foo.txt", FileOpenFlags::RDWR)
            .unwrap();
        // Seek forward pass the hello
        lfs.file_seek(&mut file, 6, Whence::Set).unwrap();
        let mut buf = [0u8; 32];
        let read_sz = lfs.file_read(&mut file, &mut buf).unwrap();
        assert_ne!(read_sz, 0);
        lfs.file_close(file).unwrap();

        lfs.unmount().unwrap();

        assert_eq!(read_sz, 6);
        assert_eq!(&buf[..6], b"world!");
    }

    #[test]
    fn test_lfs_truncate() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(
            &mut file,
            "/foo.txt",
            FileOpenFlags::RDWR | FileOpenFlags::CREAT,
        )
        .unwrap();
        let write_sz = lfs.file_write(&mut file, b"hello world!").unwrap();
        assert_ne!(write_sz, 0);

        lfs.file_truncate(&mut file, 0).unwrap();
        lfs.file_close(file).unwrap();

        let mut file = Default::default();
        lfs.file_open(&mut file, "/foo.txt", FileOpenFlags::RDWR)
            .unwrap();
        let mut buf = [0u8; 32];
        let read_sz = lfs.file_read(&mut file, &mut buf).unwrap();
        assert_eq!(read_sz, 0);
        lfs.file_close(file).unwrap();

        lfs.unmount().unwrap();
    }

    #[test]
    fn test_lfs_tell() {
        let storage = RamStorage::default();
        let mut lfs = LittleFs::new(storage);
        lfs.format().unwrap();
        lfs.mount().unwrap();
        let mut file = Default::default();
        lfs.file_open(
            &mut file,
            "/foo.txt",
            FileOpenFlags::RDWR | FileOpenFlags::CREAT,
        )
        .unwrap();
        let write_sz = lfs.file_write(&mut file, b"hello world!").unwrap();
        assert_ne!(write_sz, 0);

        let tell_sz = lfs.file_tell(&mut file).unwrap();
        assert_eq!(tell_sz, write_sz);

        lfs.file_rewind(&mut file).unwrap();
        let tell_sz = lfs.file_tell(&mut file).unwrap();
        assert_eq!(tell_sz, 0);

        lfs.file_close(file).unwrap();
        lfs.unmount().unwrap();
    }

    #[test]
    fn test_lfs_info_into_info() {
        let lfs_info = lfs::lfs_info {
            type_: lfs::lfs_type_LFS_TYPE_REG as u8,
            size: 4,
            name: [0; (NAME_MAX_LEN) + 1],
        };

        let info: Info = lfs_info.into();
        assert_eq!(info.entry_type, EntryType::RegularFile);
        assert_eq!(info.size, 4);
    }
}
