use super::{Config, File, FileId, FileManager};
use crate::DriveFacade;
use drive3;
use failure::Error;
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyStatfs, ReplyWrite, Request,
};
use libc::{ENOENT, ENOTDIR, ENOTRECOVERABLE, EREMOTE, EROFS};
use lru_time_cache::LruCache;
use std;
use std::clone::Clone;
use std::cmp;
use std::ffi::OsStr;

pub type Inode = u64;

const TRASH_INODE: Inode = 2;

macro_rules! log_result {
    ($expr:expr) => {
        match $expr {
            Ok(t) => {
                debug!("{:?}", t);
            }
            Err(e) => {
                error!("{:?}", e);
            }
        }
    };
}

macro_rules! log_result_and_fill_reply {
    ($expr:expr,$reply:ident) => {
        match $expr {
            Ok(t) => {
                debug!("{:?}", t);
                $reply.ok();
            }
            Err(e) => {
                error!("{:?}", e);
                $reply.error(ENOTRECOVERABLE);
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
            $reply.error(EROFS);
            return;
        }
    };
}

/// An empty FUSE file system. It can be used in a mounting test aimed to determine whether or
/// not the real file system can be mounted as well. If the test fails, the application can fail
/// early instead of wasting time constructing the real file system.
pub struct NullFs;
impl Filesystem for NullFs {}

/// A FUSE file system which is linked to a Google Drive account.
pub struct Gcsf {
    manager: FileManager,
    statfs_cache: LruCache<String, u64>,
    read_only: bool,
}

const TTL: std::time::Duration = std::time::Duration::from_secs(1);

impl Gcsf {
    /// Constructs a Gcsf instance using a given Config.
    pub fn with_config(config: Config) -> Result<Self, Error> {
        Ok(Gcsf {
            manager: FileManager::with_drive_facade(
                config.rename_identical_files(),
                config.add_extensions_to_special_files(),
                config.skip_trash(),
                config.sync_interval(),
                DriveFacade::new(&config),
            )?,
            statfs_cache: LruCache::<String, u64>::with_expiry_duration_and_capacity(
                config.cache_statfs_seconds(),
                2,
            ),
            read_only: config.read_only(),
        })
    }
}

impl Filesystem for Gcsf {
    fn lookup(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEntry) {
        // self.manager.sync();

        let name = name.to_str().unwrap().to_string();
        let id = FileId::ParentAndName { parent, name };

        match self.manager.get_file(&id) {
            Some(file) => {
                reply.entry(&TTL, &file.attr, 0);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn getattr(&mut self, _req: &Request, ino: Inode, _fh: Option<u64>, reply: ReplyAttr) {
        // self.manager.sync();
        match self.manager.get_file(&FileId::Inode(ino)) {
            Some(file) => {
                reply.attr(&TTL, &file.attr);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: Inode,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if !self.manager.contains(&FileId::Inode(ino)) {
            reply.error(ENOENT);
            return;
        }

        let (mime, id) = self
            .manager
            .get_file(&FileId::Inode(ino))
            .map(|f| {
                let mime = f
                    .drive_file
                    .as_ref()
                    .and_then(|f| f.mime_type.as_ref())
                    .cloned();
                let id = f.drive_id().unwrap();

                (mime, id)
            })
            .unwrap();

        reply.data(
            self.manager
                .df
                .read(&id, mime, offset as usize, size as usize)
                .unwrap_or(&[]),
        );
    }

    fn write(
        &mut self,
        _req: &Request,
        ino: Inode,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        reject_if_readonly!(self, reply);
        let offset: usize = cmp::max(offset, 0) as usize;
        self.manager.write(FileId::Inode(ino), offset, data);

        match self.manager.get_mut_file(&FileId::Inode(ino)) {
            Some(ref mut file) => {
                file.attr.size = offset as u64 + data.len() as u64;
                reply.written(data.len() as u32);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: Inode,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if let Err(e) = self.manager.sync() {
            debug!("Could not perform sync: {}", e);
        }
        // println!("current state: {:#?}", self.manager);

        let mut curr_offs = offset + 1;
        match self.manager.get_children(&FileId::Inode(ino)) {
            Some(children) => {
                for child in children.iter().skip(offset as usize) {
                    if reply.add(child.inode(), curr_offs, child.kind(), child.name()) {
                        break;
                    } else {
                        curr_offs += 1;
                    }
                }
                reply.ok();
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn rename(
        &mut self,
        _req: &Request,
        parent: Inode,
        name: &OsStr,
        newparent: Inode,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        reject_if_readonly!(self, reply);
        let name = name.to_str().unwrap().to_string();
        let newname = newname.to_str().unwrap().to_string();

        let id = FileId::Inode(
            self.manager
                .get_inode(&FileId::ParentAndName { parent, name })
                .unwrap_or(0),
        );

        if newparent == TRASH_INODE {
            let rename_res = self.manager.rename(&id, parent, newname);
            log_result!(&rename_res);

            let trash_res = self.manager.move_file_to_trash(&id, true);
            log_result!(&trash_res);

            if rename_res.is_ok() && trash_res.is_ok() {
                reply.ok();
            } else {
                reply.error(EREMOTE);
            }
        } else {
            log_result_and_fill_reply!(self.manager.rename(&id, newparent, newname), reply);
        }
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: Inode,
        _mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<u64>,
        crtime: Option<std::time::SystemTime>,
        chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        reject_if_readonly!(self, reply);
        if !self.manager.contains(&FileId::Inode(ino)) {
            error!("setattr: could not find inode={} in the file tree", ino);
            reply.error(ENOENT);
            return;
        }

        let file = self.manager.get_mut_file(&FileId::Inode(ino)).unwrap();

        let new_attr = FileAttr {
            ino: file.attr.ino,
            kind: file.attr.kind,
            size: size.unwrap_or(file.attr.size),
            blocks: file.attr.blocks,
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
            flags: flags.unwrap_or(file.attr.flags),
        };

        file.attr = new_attr;
        reply.attr(&TTL, &file.attr);
    }

    fn create(
        &mut self,
        req: &Request,
        parent: Inode,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        reject_if_readonly!(self, reply);
        let filename = name.to_str().unwrap().to_string();

        // TODO: these two checks might not be necessary
        if !self.manager.contains(&FileId::Inode(parent)) {
            error!(
                "create: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }
        if self.manager.contains(&FileId::ParentAndName {
            parent,
            name: filename.clone(),
        }) {
            error!(
                "create: file {:?} of parent(inode={}) already exists",
                name, parent
            );
            reply.error(ENOTDIR);
            return;
        }

        let file = File {
            name: filename.clone(),
            attr: FileAttr {
                ino: self.manager.next_available_inode(),
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
                parents: Some(vec![
                    self.manager.get_drive_id(&FileId::Inode(parent)).unwrap(),
                ]),
                ..Default::default()
            }),
        };

        let attr = file.attr;
        match self.manager.create_file(file, Some(FileId::Inode(parent))) {
            Ok(()) => {
                reply.created(&TTL, &attr, 0, 0, 0);
            }
            Err(e) => {
                error!("create: {}", e);
                reply.error(EREMOTE);
            }
        }
    }

    fn unlink(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        reject_if_readonly!(self, reply);
        let id = FileId::ParentAndName {
            parent,
            name: name.to_str().unwrap().to_string(),
        };

        if !self.manager.contains(&id) {
            reply.error(ENOENT);
            return;
        }

        match self.manager.file_is_trashed(&id) {
            Ok(trashed) => {
                let res = if trashed {
                    debug!("{:?} is already trashed. Deleting permanently.", id);
                    self.manager.delete(&id)
                } else if self.manager.skip_trash {
                    debug!(
                        "{:?} was not trashed. Deleting it permanently instead of moving to Trash \
                    because skip_trash is enabled in the configuration.",
                        id
                    );
                    self.manager.delete(&id)
                } else {
                    debug!(
                        "{:?} was not trashed. Moving it to Trash instead of deleting permanently.",
                        id
                    );
                    self.manager.move_file_to_trash(&id, true)
                };

                log_result_and_fill_reply!(res, reply);
            }
            Err(e) => {
                error!("{:?}", e);
                reply.error(EREMOTE);
            }
        }
    }

    fn forget(&mut self, _req: &Request, _ino: u64, _nlookup: u64) {}

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: Inode,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        reject_if_readonly!(self, reply);
        let dirname = name.to_str().unwrap().to_string();

        // TODO: these two checks might not be necessary
        if !self.manager.contains(&FileId::Inode(parent)) {
            error!(
                "mkdir: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }
        if self.manager.contains(&FileId::ParentAndName {
            parent,
            name: dirname.clone(),
        }) {
            error!(
                "mkdir: file {:?} of parent(inode={}) already exists",
                name, parent
            );
            reply.error(ENOTDIR);
            return;
        }

        let dir = File {
            name: dirname.clone(),
            attr: FileAttr {
                ino: self.manager.next_available_inode(),
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
                parents: Some(vec![
                    self.manager.get_drive_id(&FileId::Inode(parent)).unwrap(),
                ]),
                ..Default::default()
            }),
        };

        let attr = dir.attr;
        match self.manager.create_file(dir, Some(FileId::Inode(parent))) {
            Ok(()) => {
                reply.entry(&TTL, &attr, 0);
            }
            Err(e) => {
                error!("mkdir: {}", e);
                reply.error(EREMOTE);
            }
        }
    }

    fn rmdir(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        reject_if_readonly!(self, reply);
        self.unlink(_req, parent, name, reply);
    }

    fn flush(&mut self, _req: &Request, ino: Inode, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        if self.read_only {
            // In read-only mode, there are no pending writes, so flush is a no-op
            reply.ok();
            return;
        }
        match self.manager.flush(&FileId::Inode(ino)) {
            Ok(()) => reply.ok(),
            Err(e) => {
                error!("{:?}", e);
                reply.error(EREMOTE);
            }
        }
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        let (size, capacity) = if !self.statfs_cache.contains_key("size")
            || !self.statfs_cache.contains_key("capacity")
        {
            let (size, capacity) = self.manager.df.size_and_capacity().unwrap_or((0, Some(0)));
            let capacity = capacity.unwrap_or(i64::MAX as u64);
            self.statfs_cache.insert("size".to_string(), size);
            self.statfs_cache.insert("capacity".to_string(), capacity);

            (size, capacity)
        } else {
            // unwrap_or(&0) because the values might have been dropped from the cache since
            // checking for their existence.
            let size = self.statfs_cache.get("size").unwrap_or(&0).to_owned();
            let capacity = self.statfs_cache.get("capacity").unwrap_or(&0).to_owned();
            (size, capacity)
        };

        let bsize: u32 = 512;
        let blocks: u64 =
            capacity / (bsize as u64) + if capacity % (bsize as u64) > 0 { 1 } else { 0 };
        let bfree: u64 = (capacity - size) / (bsize as u64);

        reply.statfs(
            /* blocks:*/ blocks,
            /* bfree: */ bfree,
            /* bavail: */ bfree,
            /* files: */ u64::MAX,
            /* ffree: */ u64::MAX - self.manager.files.len() as u64,
            /* bsize: */ bsize,
            /* namelen: */ 1024,
            /* frsize: */ bsize,
        );
    }
}
