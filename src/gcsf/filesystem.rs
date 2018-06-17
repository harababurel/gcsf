use super::{Config, File, FileId, FileManager};
use drive3;
use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyStatfs, ReplyWrite, Request};
use libc::{ENOENT, ENOTDIR, EREMOTEIO};
use lru_time_cache::LruCache;
use std;
use std::clone::Clone;
use std::cmp;
use std::ffi::OsStr;
use time::Timespec;
use DriveFacade;

pub type Inode = u64;
pub type DriveId = String;

pub struct NullFS;
impl Filesystem for NullFS {}

pub struct GCSF {
    manager: FileManager,
    statfs_cache: LruCache<String, u64>,
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 }; // 1 second

impl GCSF {
    pub fn with_config(config: Config) -> Self {
        GCSF {
            manager: FileManager::with_drive_facade(
                config.sync_interval(),
                DriveFacade::new(&config),
            ),
            statfs_cache: LruCache::<String, u64>::with_expiry_duration_and_capacity(
                config.cache_statfs_seconds(),
                2,
            ),
        }
    }
}

impl Filesystem for GCSF {
    fn lookup(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEntry) {
        // self.manager.sync();

        let name = name.to_str().unwrap().to_string();
        let id = FileId::ParentAndName { parent, name };

        match self.manager.get_file(&id) {
            Some(ref file) => {
                reply.entry(&TTL, &file.attr, 0);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn getattr(&mut self, _req: &Request, ino: Inode, reply: ReplyAttr) {
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
        reply: ReplyData,
    ) {
        if !self.manager.contains(&FileId::Inode(ino)) {
            reply.error(ENOENT);
            return;
        }

        let (mime, id) = self.manager
            .get_file(&FileId::Inode(ino))
            .map(|f| {
                let mime = f.drive_file
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
        _flags: u32,
        reply: ReplyWrite,
    ) {
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
            warn!("Could not perform sync: {}", e);
        }

        let mut curr_offs = offset + 1;
        match self.manager.get_children(&FileId::Inode(ino)) {
            Some(children) => {
                for child in children.iter().skip(offset as usize) {
                    reply.add(child.inode(), curr_offs, child.kind(), &child.name());
                    curr_offs += 1;
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
        new_parent: Inode,
        new_name: &OsStr,
        reply: ReplyEmpty,
    ) {
        let name = name.to_str().unwrap().to_string();
        let new_name = new_name.to_str().unwrap().to_string();

        match self.manager.rename(
            &FileId::ParentAndName { parent, name },
            new_parent,
            new_name,
        ) {
            Ok(_) => {
                reply.ok();
            }
            Err(e) => {
                error!("{}", e);
                reply.error(ENOENT);
            }
        };
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: Inode,
        _mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<Timespec>,
        mtime: Option<Timespec>,
        _fh: Option<u64>,
        crtime: Option<Timespec>,
        chgtime: Option<Timespec>,
        _bkuptime: Option<Timespec>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
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
            atime: atime.unwrap_or(file.attr.atime),
            mtime: mtime.unwrap_or(file.attr.mtime),
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
        _flags: u32,
        reply: ReplyCreate,
    ) {
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
                atime: Timespec::new(1, 0),
                mtime: Timespec::new(1, 0),
                ctime: Timespec::new(1, 0),
                crtime: Timespec::new(1, 0),
                perm: 0o744,
                nlink: 0,
                uid: req.uid(),
                gid: req.gid(),
                rdev: 0,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive3::File {
                name: Some(filename.clone()),
                mime_type: None,
                parents: Some(vec![
                    self.manager.get_drive_id(&FileId::Inode(parent)).unwrap(),
                ]),
                ..Default::default()
            }),
        };

        let attr = file.attr.clone();
        match self.manager.create_file(file, Some(FileId::Inode(parent))) {
            Ok(()) => {
                reply.created(&TTL, &attr, 0, 0, 0);
            }
            Err(e) => {
                error!("create: {}", e);
                reply.error(EREMOTEIO);
            }
        }
    }

    fn unlink(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        let id = FileId::ParentAndName {
            parent,
            name: name.to_str().unwrap().to_string(),
        };

        if !self.manager.contains(&id) {
            reply.error(ENOENT);
            return;
        }

        match self.manager.move_file_to_trash(&id, true) {
            Ok(response) => {
                debug!("{:?}", response);
                reply.ok();
            }
            Err(e) => {
                error!("{:?}", e);
                reply.error(EREMOTEIO);
            }
        };
    }

    fn forget(&mut self, _req: &Request, _ino: u64, _nlookup: u64) {}

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: Inode,
        name: &OsStr,
        _mode: u32,
        reply: ReplyEntry,
    ) {
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
                size: 0,
                blocks: 123,
                atime: Timespec::new(1, 0),
                mtime: Timespec::new(1, 0),
                ctime: Timespec::new(1, 0),
                crtime: Timespec::new(1, 0),
                perm: 0o644,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive3::File {
                name: Some(dirname.clone()),
                mime_type: Some("application/vnd.google-apps.folder".to_string()),
                parents: Some(vec![
                    self.manager.get_drive_id(&FileId::Inode(parent)).unwrap(),
                ]),
                ..Default::default()
            }),
        };

        let attr = dir.attr.clone();
        match self.manager.create_file(dir, Some(FileId::Inode(parent))) {
            Ok(()) => {
                reply.entry(&TTL, &attr, 0);
            }
            Err(e) => {
                error!("mkdir: {}", e);
                reply.error(EREMOTEIO);
            }
        }
    }

    fn rmdir(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        match self.manager.delete(&FileId::ParentAndName {
            parent,
            name: name.to_str().unwrap().to_string(),
        }) {
            Ok(response) => {
                debug!("{:?}", response);
                reply.ok();
            }
            Err(e) => {
                error!("{:?}", e);
                reply.error(EREMOTEIO);
            }
        };
    }

    fn flush(&mut self, _req: &Request, ino: Inode, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        match self.manager.flush(&FileId::Inode(ino)) {
            Ok(()) => reply.ok(),
            Err(e) => {
                error!("{:?}", e);
                reply.error(EREMOTEIO);
            }
        }
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        let (size, capacity) = if !self.statfs_cache.contains_key("size")
            || !self.statfs_cache.contains_key("capacity")
        {
            let (size, capacity) = self.manager.df.size_and_capacity().unwrap_or((0, Some(0)));
            let capacity = capacity.unwrap_or(std::i64::MAX as u64);
            self.statfs_cache.insert("size".to_string(), size);
            self.statfs_cache.insert("capacity".to_string(), capacity);

            (size, capacity)
        } else {
            let size = self.statfs_cache.get("size").unwrap().to_owned();
            let capacity = self.statfs_cache.get("capacity").unwrap().to_owned();
            (size, capacity)
        };

        reply.statfs(
            /* blocks:*/ capacity,
            /* bfree: */ capacity - size,
            /* bavail: */ capacity - size,
            /* files: */ std::u64::MAX,
            /* ffree: */ std::u64::MAX - self.manager.files.len() as u64,
            /* bsize: */ 1,
            /* namelen: */ 1024,
            /* frsize: */ 1,
        );
    }
}
