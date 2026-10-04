use super::{Config, DriveAttributes, File, FileId, FileManager};
use crate::DriveFacade;
use drive3;
use failure::{Error, err_msg};
use fuser::{
    AccessFlags, BsdFileFlags, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags,
    Generation, INodeNo, LockOwner, OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyStatfs, ReplyWrite, ReplyXattr, Request,
    WriteFlags,
};
use lru_time_cache::LruCache;
use std;
use std::clone::Clone;
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub type Inode = u64;

const TRASH_INODE: Inode = 2;

macro_rules! log_result_and_fill_reply {
    ($expr:expr,$reply:ident) => {
        match $expr {
            Ok(t) => {
                debug!("{:?}", t);
                $reply.ok();
            }
            Err(e) => {
                error!("{:?}", e);
                $reply.error(Errno::EIO);
                return;
            }
        }
    };
}

/// Returns EROFS if filesystem is in read-only mode
macro_rules! reject_if_readonly {
    ($self:ident, $reply:ident) => {
        if $self.read_only {
            warn!("Rejecting write operation: filesystem is read-only");
            $reply.error(Errno::EROFS);
            return;
        } else if $self.shutting_down.load(Ordering::Acquire) {
            warn!("Rejecting write operation: filesystem is shutting down");
            $reply.error(Errno::EIO);
            return;
        }
    };
}

macro_rules! lock_state {
    ($self:ident, $reply:ident) => {
        match $self.state() {
            Ok(state) => state,
            Err(error) => {
                $reply.error(error);
                return;
            }
        }
    };
}

/// An empty FUSE file system. It can be used in a mounting test aimed to determine whether or
/// not the real file system can be mounted as well. If the test fails, the application can fail
/// early instead of wasting time constructing the real file system.
pub struct NullFs;
impl Filesystem for NullFs {}

struct GcsfState {
    manager: FileManager,
    statfs_cache: LruCache<String, u64>,
}

/// A FUSE file system which is linked to a Google Drive account.
pub struct Gcsf {
    state: Arc<Mutex<GcsfState>>,
    read_only: bool,
    shutting_down: Arc<AtomicBool>,
}

/// A handle used by the mount process to stop writes and flush pending data safely.
pub struct GcsfControl {
    state: Arc<Mutex<GcsfState>>,
    shutting_down: Arc<AtomicBool>,
}

impl GcsfControl {
    /// Rejects future writes and flushes all operations accepted before shutdown began.
    pub fn begin_shutdown(&self) -> Result<(), Error> {
        self.shutting_down.store(true, Ordering::Release);
        self.flush_pending()
    }

    /// Flushes all pending file operations to Drive.
    pub fn flush_pending(&self) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| err_msg("Filesystem state lock was poisoned during shutdown"))?;
        state.manager.flush_all()
    }
}

const TTL: std::time::Duration = std::time::Duration::from_secs(1);

/// The extended attributes (name, value) exposing a file's Drive metadata. Attributes whose
/// value Drive does not provide for the file are omitted.
fn drive_xattrs(attributes: &DriveAttributes) -> Vec<(&'static str, &str)> {
    let content = &attributes.content;
    [
        ("user.gcsf.id", &attributes.id),
        ("user.gcsf.mime_type", &content.mime_type),
        ("user.gcsf.owner", &attributes.owners),
        ("user.gcsf.web_link", &attributes.web_link),
        ("user.gcsf.md5", &content.md5),
        ("user.gcsf.sha1", &content.sha1),
        ("user.gcsf.sha256", &content.sha256),
        ("user.gcsf.revision", &content.revision),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.as_deref().map(|value| (name, value)))
    .collect()
}

/// The `listxattr` payload: every attribute name followed by a NUL byte.
fn xattr_name_list(attributes: &DriveAttributes) -> Vec<u8> {
    drive_xattrs(attributes)
        .into_iter()
        .flat_map(|(name, _)| name.bytes().chain(std::iter::once(0)))
        .collect()
}

/// Answers an xattr request following the FUSE protocol: a `size` of 0 asks for the length of
/// the value, otherwise the value is returned if it fits in `size` bytes.
fn reply_xattr(reply: ReplyXattr, size: u32, value: &[u8]) {
    let Ok(len) = u32::try_from(value.len()) else {
        reply.error(Errno::E2BIG);
        return;
    };
    if size == 0 {
        reply.size(len);
    } else if len > size {
        reply.error(Errno::ERANGE);
    } else {
        reply.data(value);
    }
}

impl Gcsf {
    /// Constructs a Gcsf instance using a given Config.
    pub fn with_config(config: Config) -> Result<Self, Error> {
        let manager = FileManager::with_drive_facade(
            config.rename_identical_files(),
            config.add_extensions_to_special_files(),
            config.skip_trash(),
            config.sync_interval(),
            DriveFacade::new(&config),
        )?;
        Ok(Gcsf {
            state: Arc::new(Mutex::new(GcsfState {
                manager,
                statfs_cache: LruCache::<String, u64>::with_expiry_duration_and_capacity(
                    config.cache_statfs_seconds(),
                    2,
                ),
            })),
            read_only: config.read_only(),
            shutting_down: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Returns a handle that can freeze writes and flush pending data during unmount.
    pub fn control(&self) -> GcsfControl {
        GcsfControl {
            state: Arc::clone(&self.state),
            shutting_down: Arc::clone(&self.shutting_down),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, GcsfState>, Errno> {
        let state = self.state.lock().map_err(|_| {
            error!("Filesystem state lock was poisoned; refusing to continue");
            Errno::EIO
        })?;
        if self.shutting_down.load(Ordering::Acquire) {
            Err(Errno::EIO)
        } else {
            Ok(state)
        }
    }

    fn remove_file(manager: &mut FileManager, id: &FileId) -> Result<(), Error> {
        if manager.file_is_trashed(id)? {
            debug!("{:?} is already trashed. Deleting permanently.", id);
            manager.delete(id)
        } else if manager.skip_trash {
            debug!(
                "{:?} was not trashed. Deleting it permanently because skip_trash is enabled.",
                id
            );
            manager.delete(id)
        } else {
            debug!("{:?} was not trashed. Moving it to Trash.", id);
            manager.move_file_to_trash(id, true)
        }
    }
}

impl Filesystem for Gcsf {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let state = lock_state!(self, reply);
        let id = FileId::ParentAndName {
            parent: parent.0,
            name: name.to_string(),
        };

        match state.manager.get_file(&id) {
            Some(file) => {
                reply.entry(&TTL, &file.attr, Generation(0));
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        };
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let state = lock_state!(self, reply);
        match state.manager.get_file(&FileId::Inode(ino.0)) {
            Some(file) => {
                reply.attr(&TTL, &file.attr);
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        };
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        let state = lock_state!(self, reply);
        if !state.manager.contains(&FileId::Inode(ino.0)) {
            reply.error(Errno::ENOENT);
        } else if self.read_only && mask.contains(AccessFlags::W_OK) {
            reply.error(Errno::EROFS);
        } else {
            reply.ok();
        }
    }

    fn getxattr(&self, _req: &Request, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        let state = lock_state!(self, reply);
        let id = FileId::Inode(ino.0);
        if !state.manager.contains(&id) {
            reply.error(Errno::ENOENT);
            return;
        }
        let attributes = state.manager.drive_attributes(&id).unwrap_or_default();
        let value = drive_xattrs(&attributes)
            .into_iter()
            .find(|(xattr_name, _)| OsStr::new(xattr_name) == name)
            .map(|(_, value)| value);
        match value {
            Some(value) => reply_xattr(reply, size, value.as_bytes()),
            None => reply.error(Errno::NO_XATTR),
        }
    }

    fn listxattr(&self, _req: &Request, ino: INodeNo, size: u32, reply: ReplyXattr) {
        let state = lock_state!(self, reply);
        let id = FileId::Inode(ino.0);
        if !state.manager.contains(&id) {
            reply.error(Errno::ENOENT);
            return;
        }
        let attributes = state.manager.drive_attributes(&id).unwrap_or_default();
        reply_xattr(reply, size, &xattr_name_list(&attributes));
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let Ok(offset) = usize::try_from(offset) else {
            reply.error(Errno::EOVERFLOW);
            return;
        };
        let mut state = lock_state!(self, reply);
        let id = FileId::Inode(ino.0);
        let Some(file) = state.manager.get_file(&id) else {
            reply.error(Errno::ENOENT);
            return;
        };

        let mime = file
            .drive_file
            .as_ref()
            .and_then(|file| file.mime_type.as_ref())
            .cloned();
        let Some(drive_id) = file.drive_id() else {
            reply.error(Errno::EIO);
            return;
        };

        match state
            .manager
            .df
            .read(&drive_id, mime, offset, size as usize)
        {
            Some(data) => reply.data(data),
            // Returning an empty buffer here would be indistinguishable from a
            // genuinely empty file, silently hiding the failure from the caller.
            None => reply.error(Errno::EIO),
        }
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        reject_if_readonly!(self, reply);
        let Ok(offset) = usize::try_from(offset) else {
            reply.error(Errno::EOVERFLOW);
            return;
        };
        let Some(end) = offset.checked_add(data.len()) else {
            reply.error(Errno::EOVERFLOW);
            return;
        };
        let mut state = lock_state!(self, reply);
        let id = FileId::Inode(ino.0);
        if !state.manager.contains(&id) {
            reply.error(Errno::ENOENT);
            return;
        }
        let synchronous = flags.0 & (libc::O_SYNC | libc::O_DSYNC) != 0;
        let write_result = if synchronous {
            state.manager.write_and_flush(&id, offset, data)
        } else {
            state.manager.write(&id, offset, data)
        };
        if let Err(error) = write_result {
            error!("write: {}", error);
            reply.error(Errno::EIO);
            return;
        }

        let Some(file) = state.manager.get_mut_file(&id) else {
            reply.error(Errno::EIO);
            return;
        };
        file.attr.size = file.attr.size.max(end as u64);
        file.attr.blocks = file.attr.size.div_ceil(512);
        reply.written(data.len() as u32);
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let mut state = lock_state!(self, reply);
        if let Err(e) = state.manager.sync() {
            debug!("Could not perform sync: {}", e);
        }

        let mut curr_offs = offset.saturating_add(1);
        match state.manager.get_children(&FileId::Inode(ino.0)) {
            Some(children) => {
                let skip = usize::try_from(offset).unwrap_or(usize::MAX);
                for child in children.iter().skip(skip) {
                    if reply.add(child.attr.ino, curr_offs, child.kind(), child.name()) {
                        break;
                    } else {
                        curr_offs += 1;
                    }
                }
                reply.ok();
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        };
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        reject_if_readonly!(self, reply);
        #[cfg(target_os = "linux")]
        let flags_supported = flags.is_empty() || flags == RenameFlags::RENAME_NOREPLACE;
        #[cfg(not(target_os = "linux"))]
        let flags_supported = flags.is_empty();
        if !flags_supported {
            reply.error(Errno::EINVAL);
            return;
        }
        let (Some(name), Some(newname)) = (name.to_str(), newname.to_str()) else {
            reply.error(Errno::EINVAL);
            return;
        };
        let mut state = lock_state!(self, reply);
        let parent = parent.0;
        let newparent = newparent.0;
        let name = name.to_string();
        let newname = newname.to_string();

        let Some(inode) = state
            .manager
            .get_inode(&FileId::ParentAndName { parent, name })
        else {
            reply.error(Errno::ENOENT);
            return;
        };
        let id = FileId::Inode(inode);
        let destination = FileId::ParentAndName {
            parent: newparent,
            name: newname.clone(),
        };
        if let Some(destination_inode) = state.manager.get_inode(&destination) {
            if destination_inode == inode {
                reply.ok();
            } else {
                // Replacing a Drive object cannot be made atomic. Fail safely rather
                // than creating two entries with the same POSIX name.
                reply.error(Errno::EEXIST);
            }
            return;
        }

        if newparent == TRASH_INODE {
            log_result_and_fill_reply!(state.manager.rename_and_move_to_trash(&id, newname), reply);
        } else {
            log_result_and_fill_reply!(state.manager.rename(&id, newparent, newname), reply);
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<FileHandle>,
        crtime: Option<std::time::SystemTime>,
        chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        reject_if_readonly!(self, reply);
        let mut state = lock_state!(self, reply);
        let id = FileId::Inode(ino.0);
        if !state.manager.contains(&id) {
            error!("setattr: could not find inode={} in the file tree", ino.0);
            reply.error(Errno::ENOENT);
            return;
        }

        if let Some(size) = size {
            let Ok(size) = usize::try_from(size) else {
                reply.error(Errno::EOVERFLOW);
                return;
            };
            if let Err(error) = state.manager.truncate(&id, size) {
                error!("setattr: could not stage truncate: {}", error);
                reply.error(Errno::EIO);
                return;
            }
        }

        let Some(file) = state.manager.get_mut_file(&id) else {
            reply.error(Errno::EIO);
            return;
        };

        let new_attr = FileAttr {
            ino: file.attr.ino,
            kind: file.attr.kind,
            size: size.unwrap_or(file.attr.size),
            blocks: size.unwrap_or(file.attr.size).div_ceil(512),
            blksize: file.attr.blksize,
            atime: match atime.unwrap_or(fuser::TimeOrNow::SpecificTime(file.attr.atime)) {
                fuser::TimeOrNow::SpecificTime(t) => t,
                fuser::TimeOrNow::Now => std::time::SystemTime::now(),
            },
            mtime: match mtime.unwrap_or(fuser::TimeOrNow::SpecificTime(file.attr.mtime)) {
                fuser::TimeOrNow::SpecificTime(t) => t,
                fuser::TimeOrNow::Now => std::time::SystemTime::now(),
            },
            ctime: chgtime.unwrap_or(file.attr.ctime),
            crtime: crtime.unwrap_or(file.attr.crtime),
            perm: file.attr.perm,
            nlink: file.attr.nlink,
            uid: uid.unwrap_or(file.attr.uid),
            gid: gid.unwrap_or(file.attr.gid),
            rdev: file.attr.rdev,
            flags: flags.map_or(file.attr.flags, |flags| flags.bits()),
        };

        file.attr = new_attr;
        reply.attr(&TTL, &file.attr);
    }

    fn create(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        reject_if_readonly!(self, reply);
        let Some(filename) = name.to_str().map(str::to_string) else {
            reply.error(Errno::EINVAL);
            return;
        };
        let mut state = lock_state!(self, reply);
        let parent = parent.0;

        // TODO: these two checks might not be necessary
        if !state.manager.contains(&FileId::Inode(parent)) {
            error!(
                "create: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(Errno::ENOENT);
            return;
        }
        if state.manager.contains(&FileId::ParentAndName {
            parent,
            name: filename.clone(),
        }) {
            error!(
                "create: file {:?} of parent(inode={}) already exists",
                name, parent
            );
            reply.error(Errno::EEXIST);
            return;
        }

        let Some(parent_drive_id) = state.manager.get_drive_id(&FileId::Inode(parent)) else {
            reply.error(Errno::EIO);
            return;
        };

        let file = File {
            name: filename.clone(),
            attr: FileAttr {
                ino: INodeNo(state.manager.next_available_inode()),
                kind: FileType::RegularFile,
                size: 0,
                blocks: 123,
                blksize: 512,
                atime: std::time::SystemTime::now(),
                mtime: std::time::SystemTime::now(),
                ctime: std::time::SystemTime::now(),
                crtime: std::time::SystemTime::now(),
                perm: 0o744,
                nlink: 0,
                uid: req.uid(),
                gid: req.gid(),
                rdev: 0,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive3::api::File {
                name: Some(filename),
                mime_type: None,
                parents: Some(vec![parent_drive_id]),
                ..Default::default()
            }),
        };

        let attr = file.attr;
        match state.manager.create_file(file, Some(FileId::Inode(parent))) {
            Ok(()) => {
                reply.created(
                    &TTL,
                    &attr,
                    Generation(0),
                    FileHandle(0),
                    FopenFlags::empty(),
                );
            }
            Err(e) => {
                error!("create: {}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        reject_if_readonly!(self, reply);
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let mut state = lock_state!(self, reply);
        let id = FileId::ParentAndName {
            parent: parent.0,
            name: name.to_string(),
        };

        if !state.manager.contains(&id) {
            reply.error(Errno::ENOENT);
            return;
        }
        if state.manager.get_file(&id).map(File::kind) == Some(FileType::Directory) {
            reply.error(Errno::EISDIR);
            return;
        }

        log_result_and_fill_reply!(Self::remove_file(&mut state.manager, &id), reply);
    }

    fn forget(&self, _req: &Request, _ino: INodeNo, _nlookup: u64) {}

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        reject_if_readonly!(self, reply);
        let Some(dirname) = name.to_str().map(str::to_string) else {
            reply.error(Errno::EINVAL);
            return;
        };
        let mut state = lock_state!(self, reply);
        let parent = parent.0;

        // TODO: these two checks might not be necessary
        if !state.manager.contains(&FileId::Inode(parent)) {
            error!(
                "mkdir: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(Errno::ENOENT);
            return;
        }
        if state.manager.contains(&FileId::ParentAndName {
            parent,
            name: dirname.clone(),
        }) {
            error!(
                "mkdir: file {:?} of parent(inode={}) already exists",
                name, parent
            );
            reply.error(Errno::EEXIST);
            return;
        }

        let Some(parent_drive_id) = state.manager.get_drive_id(&FileId::Inode(parent)) else {
            reply.error(Errno::EIO);
            return;
        };

        let dir = File {
            name: dirname.clone(),
            attr: FileAttr {
                ino: INodeNo(state.manager.next_available_inode()),
                kind: FileType::Directory,
                size: 512,
                blocks: 1,
                atime: std::time::SystemTime::now(),
                mtime: std::time::SystemTime::now(),
                ctime: std::time::SystemTime::now(),
                crtime: std::time::SystemTime::now(),
                blksize: 512,
                perm: 0o644,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive3::api::File {
                name: Some(dirname),
                mime_type: Some("application/vnd.google-apps.folder".to_string()),
                parents: Some(vec![parent_drive_id]),
                ..Default::default()
            }),
        };

        let attr = dir.attr;
        match state.manager.create_file(dir, Some(FileId::Inode(parent))) {
            Ok(()) => {
                reply.entry(&TTL, &attr, Generation(0));
            }
            Err(e) => {
                error!("mkdir: {}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        reject_if_readonly!(self, reply);
        let Some(name) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let mut state = lock_state!(self, reply);
        let id = FileId::ParentAndName {
            parent: parent.0,
            name: name.to_string(),
        };
        let Some(file) = state.manager.get_file(&id) else {
            reply.error(Errno::ENOENT);
            return;
        };
        if file.kind() != FileType::Directory {
            reply.error(Errno::ENOTDIR);
            return;
        }
        if state
            .manager
            .get_children(&id)
            .is_some_and(|children| !children.is_empty())
        {
            reply.error(Errno::ENOTEMPTY);
            return;
        }

        log_result_and_fill_reply!(Self::remove_file(&mut state.manager, &id), reply);
    }

    fn flush(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        if self.read_only {
            // In read-only mode, there are no pending writes, so flush is a no-op
            reply.ok();
            return;
        }
        let mut state = lock_state!(self, reply);
        match state.manager.flush(&FileId::Inode(ino.0)) {
            Ok(()) => reply.ok(),
            Err(e) => {
                error!("{:?}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn fsync(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        if self.read_only {
            reply.ok();
            return;
        }
        let mut state = lock_state!(self, reply);
        match state.manager.flush(&FileId::Inode(ino.0)) {
            Ok(()) => reply.ok(),
            Err(error) => {
                error!("fsync: {:?}", error);
                reply.error(Errno::EIO);
            }
        }
    }

    fn release(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        if self.read_only {
            reply.ok();
            return;
        }
        let mut state = lock_state!(self, reply);
        match state.manager.flush(&FileId::Inode(ino.0)) {
            Ok(()) => reply.ok(),
            Err(error) => {
                error!("release: {:?}", error);
                reply.error(Errno::EIO);
            }
        }
    }

    fn destroy(&mut self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                error!("Filesystem state is poisoned; attempting a final flush anyway");
                poisoned.into_inner()
            }
        };
        if let Err(error) = state.manager.flush_all() {
            let message = format!(
                "Could not flush all pending writes during unmount: {}",
                error
            );
            error!("{}", message);
        }
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let mut state = lock_state!(self, reply);
        let (size, capacity) = if !state.statfs_cache.contains_key("size")
            || !state.statfs_cache.contains_key("capacity")
        {
            let (size, capacity) = state.manager.df.size_and_capacity().unwrap_or((0, Some(0)));
            let capacity = capacity.unwrap_or(i64::MAX as u64);
            state.statfs_cache.insert("size".to_string(), size);
            state.statfs_cache.insert("capacity".to_string(), capacity);

            (size, capacity)
        } else {
            // unwrap_or(&0) because the values might have been dropped from the cache since
            // checking for their existence.
            let size = state.statfs_cache.get("size").unwrap_or(&0).to_owned();
            let capacity = state.statfs_cache.get("capacity").unwrap_or(&0).to_owned();
            (size, capacity)
        };

        let bsize: u32 = 512;
        let blocks: u64 =
            capacity / (bsize as u64) + if capacity % (bsize as u64) > 0 { 1 } else { 0 };
        let bfree: u64 = capacity.saturating_sub(size) / (bsize as u64);

        reply.statfs(
            /* blocks:*/ blocks,
            /* bfree: */ bfree,
            /* bavail: */ bfree,
            /* files: */ u64::MAX,
            /* ffree: */ u64::MAX - state.manager.files.len() as u64,
            /* bsize: */ bsize,
            /* namelen: */ 1024,
            /* frsize: */ bsize,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{drive_xattrs, xattr_name_list};
    use crate::gcsf::{ContentMetadata, DriveAttributes};

    #[test]
    fn xattrs_expose_all_available_metadata() {
        let attributes = DriveAttributes {
            id: Some("id".to_string()),
            owners: Some("me@example.com".to_string()),
            web_link: Some("https://example.com/id".to_string()),
            content: ContentMetadata {
                md5: Some("md5".to_string()),
                sha1: Some("sha1".to_string()),
                sha256: Some("sha256".to_string()),
                revision: Some("revision".to_string()),
                mime_type: Some("application/zip".to_string()),
            },
        };
        assert_eq!(
            drive_xattrs(&attributes),
            vec![
                ("user.gcsf.id", "id"),
                ("user.gcsf.mime_type", "application/zip"),
                ("user.gcsf.owner", "me@example.com"),
                ("user.gcsf.web_link", "https://example.com/id"),
                ("user.gcsf.md5", "md5"),
                ("user.gcsf.sha1", "sha1"),
                ("user.gcsf.sha256", "sha256"),
                ("user.gcsf.revision", "revision"),
            ]
        );
        assert_eq!(
            xattr_name_list(&attributes),
            b"user.gcsf.id\0user.gcsf.mime_type\0user.gcsf.owner\0user.gcsf.web_link\0\
              user.gcsf.md5\0user.gcsf.sha1\0user.gcsf.sha256\0user.gcsf.revision\0"
        );
    }

    #[test]
    fn xattrs_omit_missing_metadata() {
        let md5_only = DriveAttributes {
            content: ContentMetadata {
                md5: Some("abc".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(drive_xattrs(&md5_only), vec![("user.gcsf.md5", "abc")]);
        assert_eq!(xattr_name_list(&md5_only), b"user.gcsf.md5\0");

        assert!(drive_xattrs(&DriveAttributes::default()).is_empty());
        assert!(xattr_name_list(&DriveAttributes::default()).is_empty());
    }
}
