use super::{File, FileId, FileManager};
use GoogleDriveFetcher;
use drive3;
use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyStatfs, ReplyWrite, Request};
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use id_tree::{Node, NodeId, NodeIdError, Tree, TreeBuilder};
use libc::{ENOENT, ENOTDIR, ENOTEMPTY};
use lru_time_cache::LruCache;
use std;
use std::clone::Clone;
use std::cmp;
use std::collections::{HashMap, LinkedList};
use std::ffi::OsStr;
use std::fmt;
use time::Timespec;

pub type Inode = u64;
pub type DriveId = String;

pub struct GCSF {
    manager: FileManager,

    drive_fetcher: GoogleDriveFetcher,
    statfs_cache: LruCache<String, u64>,
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 }; // 1 second

impl GCSF {
    pub fn new() -> Self {
        let mut drive_fetcher = GoogleDriveFetcher::new();
        let mut manager = FileManager::new();

        let mut root = GCSF::new_root_file();
        root.drive_file.as_mut().unwrap().id = Some(drive_fetcher.root_id());

        manager.add_file(root.clone(), None);
        manager.add_file(
            GCSF::new_shared_with_me_file(),
            Some(FileId::Inode(root.inode())),
        );

        let mut inode = 3;
        let mut queue: LinkedList<DriveId> = LinkedList::new();
        queue.push_back(drive_fetcher.root_id());

        while !queue.is_empty() {
            let parent_id = queue.pop_front().unwrap();

            for drive_file in drive_fetcher.get_all_files(Some(&parent_id)) {
                let mut file = File::from_drive_file(inode, drive_file);

                if file.kind() == FileType::Directory {
                    queue.push_back(file.drive_id().unwrap());
                }

                // TODO: this makes everything slow; find a better solution
                if file.is_drive_document() {
                    let size = drive_fetcher
                        .get_file_size(file.drive_id().as_ref().unwrap(), file.mime_type());
                    file.attr.size = size;
                }

                if manager.contains(FileId::DriveId(parent_id.clone())) {
                    manager.add_file(file, Some(FileId::DriveId(parent_id.clone())));
                } else {
                    manager.add_file(file, None);
                }

                inode += 1;
            }
        }

        GCSF {
            manager,
            drive_fetcher,
            statfs_cache: LruCache::<String, u64>::with_expiry_duration_and_capacity(
                std::time::Duration::from_secs(5),
                2,
            ),
        }
    }

    fn new_root_file() -> File {
        File {
            name: String::from("."),
            attr: FileAttr {
                ino: 1,
                size: 0,
                blocks: 123,
                atime: Timespec { sec: 0, nsec: 0 },
                mtime: Timespec { sec: 0, nsec: 0 },
                ctime: Timespec { sec: 0, nsec: 0 },
                crtime: Timespec { sec: 0, nsec: 0 },
                kind: FileType::Directory,
                perm: 0o755,
                nlink: 2,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            drive_file: Some(drive3::File::default()),
        }
    }

    fn new_shared_with_me_file() -> File {
        File {
            name: String::from("Shared with me"),
            attr: FileAttr {
                ino: 2,
                size: 0,
                blocks: 123,
                atime: Timespec { sec: 0, nsec: 0 },
                mtime: Timespec { sec: 0, nsec: 0 },
                ctime: Timespec { sec: 0, nsec: 0 },
                crtime: Timespec { sec: 0, nsec: 0 },
                kind: FileType::Directory,
                perm: 0o755,
                nlink: 2,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            drive_file: None,
        }
    }

    // fn get_file_from_parent(&self, parent: Inode, name: &OsStr) -> Option<&File> {
    //     let name = name.to_str().unwrap();
    //     self.tree
    //         .children(self.get_node_id(parent)?)
    //         .unwrap()
    //         .map(|child| self.get_file(*child.data()).unwrap())
    //         .find(|file| file.name == name)
    // }

    // fn get_file_with_id(&self, id: &NodeId) -> Option<&File> {
    //     match self.tree.get(id) {
    //         Ok(node) => self.files.get(node.data()),
    //         Err(_e) => None,
    //     }
    // }

    // fn get_file(&self, ino: Inode) -> Option<&File> {
    //     self.files.get(&ino)
    // }

    // fn get_mut_file(&mut self, ino: Inode) -> Option<&mut File> {
    //     self.files.get_mut(&ino)
    // }

    // #[allow(dead_code)]
    // fn get_node(&self, ino: Inode) -> Result<&Node<Inode>, NodeIdError> {
    //     let node_id = self.node_ids.get(&ino).unwrap();
    //     self.tree.get(&node_id)
    // }

    // fn get_node_id(&self, ino: Inode) -> Option<&NodeId> {
    //     self.node_ids.get(&ino)
    // }

    // fn contains(&self, ino: &Inode) -> bool {
    //     self.files.contains_key(&ino)
    // }

    // fn remove(&mut self, ino: Inode) -> Result<(), &str> {
    //     let id = self.get_node_id(ino).ok_or("NodeId not found")?.clone();
    //     let _result = self.tree.remove_node(id, DropChildren);
    //     self.files.remove(&ino);
    //     self.node_ids.remove(&ino);
    //     self.drive_fetcher.remove(ino);

    //     Ok(())
    // }

    // fn next_available_inode(&self) -> Inode {
    //     (1..)
    //         .filter(|inode| !self.contains(inode))
    //         .take(1)
    //         .next()
    //         .unwrap()
    // }
}

impl Filesystem for GCSF {
    fn lookup(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEntry) {
        let name = name.to_str().unwrap().to_string();
        let id = FileId::ParentAndName { parent, name };

        match self.manager.get_file(id) {
            Some(ref file) => {
                reply.entry(&TTL, &file.attr, 0);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn getattr(&mut self, _req: &Request, ino: Inode, reply: ReplyAttr) {
        match self.manager.get_file(FileId::Inode(ino)) {
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
        match self.manager.get_file(FileId::Inode(ino)) {
            Some(file) => {
                let mime_type = file.drive_file.clone().and_then(|file| file.mime_type);
                info!("mime_type: {:?}", &mime_type);

                reply.data(
                    self.drive_fetcher
                        .read(
                            &file.drive_id().unwrap(),
                            mime_type,
                            offset as usize,
                            size as usize,
                        )
                        .unwrap_or(&[]),
                );
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    // fn write(
    //     &mut self,
    //     _req: &Request,
    //     ino: Inode,
    //     _fh: u64,
    //     offset: i64,
    //     data: &[u8],
    //     _flags: u32,
    //     reply: ReplyWrite,
    // ) {
    //     let offset: usize = cmp::max(offset, 0) as usize;
    //     self.drive_fetcher.write(ino, offset, data);

    //     match self.get_mut_file(ino) {
    //         Some(ref mut file) => {
    //             file.attr.size = offset as u64 + data.len() as u64;
    //             reply.written(data.len() as u32);
    //         }
    //         None => {
    //             reply.error(ENOENT);
    //         }
    //     };
    // }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: Inode,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let mut curr_offs = offset + 1;
        match self.manager.get_children(FileId::Inode(ino)) {
            Some(children) => {
                for child in children.iter().skip(offset as usize) {
                    reply.add(child.inode(), curr_offs, child.kind(), &child.name);
                    curr_offs += 1;
                }
                reply.ok();
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    // fn rename(
    //     &mut self,
    //     _req: &Request,
    //     parent: Inode,
    //     name: &OsStr,
    //     new_parent: u64,
    //     new_name: &OsStr,
    //     reply: ReplyEmpty,
    // ) {
    //     let file_inode = self.get_file_from_parent(parent, name).unwrap().inode();

    //     // TODO: is to_owned() safe?
    //     let file_id = self.get_node_id(file_inode).unwrap().to_owned();
    //     let new_parent_id = self.get_node_id(new_parent).unwrap().to_owned();

    //     let _result = self.tree.move_node(&file_id, ToParent(&new_parent_id));
    //     self.get_mut_file(file_inode).unwrap().name = new_name.to_str().unwrap().to_string();

    //     reply.ok()
    // }

    // fn setattr(
    //     &mut self,
    //     _req: &Request,
    //     ino: Inode,
    //     _mode: Option<u32>,
    //     uid: Option<u32>,
    //     gid: Option<u32>,
    //     size: Option<u64>,
    //     atime: Option<Timespec>,
    //     mtime: Option<Timespec>,
    //     _fh: Option<u64>,
    //     crtime: Option<Timespec>,
    //     chgtime: Option<Timespec>,
    //     _bkuptime: Option<Timespec>,
    //     flags: Option<u32>,
    //     reply: ReplyAttr,
    // ) {
    //     if !self.files.contains_key(&ino) {
    //         error!("create: could not find inode={} in the file tree", ino);
    //         reply.error(ENOENT);
    //         return;
    //     }

    //     let file = self.get_mut_file(ino).unwrap();

    //     let new_attr = FileAttr {
    //         ino: file.attr.ino,
    //         kind: file.attr.kind,
    //         size: size.unwrap_or(file.attr.size),
    //         blocks: file.attr.blocks,
    //         atime: atime.unwrap_or(file.attr.atime),
    //         mtime: mtime.unwrap_or(file.attr.mtime),
    //         ctime: chgtime.unwrap_or(file.attr.ctime),
    //         crtime: crtime.unwrap_or(file.attr.crtime),
    //         perm: file.attr.perm,
    //         nlink: file.attr.nlink,
    //         uid: uid.unwrap_or(file.attr.uid),
    //         gid: gid.unwrap_or(file.attr.gid),
    //         rdev: file.attr.rdev,
    //         flags: flags.unwrap_or(file.attr.flags),
    //     };

    //     file.attr = new_attr;
    //     reply.attr(&TTL, &file.attr);
    // }

    // fn create(
    //     &mut self,
    //     req: &Request,
    //     parent: Inode,
    //     name: &OsStr,
    //     _mode: u32,
    //     _flags: u32,
    //     reply: ReplyCreate,
    // ) {
    //     if !self.files.contains_key(&parent) {
    //         error!(
    //             "create: could not find parent inode={} in the file tree",
    //             parent
    //         );
    //         reply.error(ENOTDIR);
    //         return;
    //     }

    //     let ino = self.next_available_inode();
    //     let file = File {
    //         name: name.to_str().unwrap().to_string(),
    //         attr: FileAttr {
    //             ino: ino,
    //             kind: FileType::RegularFile,
    //             size: 0,
    //             blocks: 123,
    //             atime: Timespec::new(1, 0),
    //             mtime: Timespec::new(1, 0),
    //             ctime: Timespec::new(1, 0),
    //             crtime: Timespec::new(1, 0),
    //             perm: 0o744,
    //             nlink: 0,
    //             uid: req.uid(),
    //             gid: req.gid(),
    //             rdev: 0,
    //             flags: 0,
    //         },
    //         drive_file: None,
    //     };

    //     reply.created(&TTL, &file.attr, 0, 0, 0);

    //     let parent_id = self.get_node_id(parent).unwrap().clone();
    //     let file_id: &NodeId = &self.tree
    //         .insert(Node::new(ino), UnderNode(&parent_id))
    //         .unwrap();
    //     self.files.insert(ino, file);
    //     self.node_ids.insert(ino, file_id.clone());
    // }

    // fn unlink(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
    //     let ino = self.get_file_from_parent(parent, name).unwrap().inode();

    //     match self.remove(ino) {
    //         Ok(()) => reply.ok(),
    //         Err(_e) => reply.error(ENOENT),
    //     };
    // }

    // fn mkdir(
    //     &mut self,
    //     _req: &Request,
    //     parent: Inode,
    //     name: &OsStr,
    //     _mode: u32,
    //     reply: ReplyEntry,
    // ) {
    //     if !self.files.contains_key(&parent) {
    //         error!(
    //             "mkdir: could not find parent inode={} in the file tree",
    //             parent
    //         );
    //         reply.error(ENOTDIR);
    //         return;
    //     }

    //     let ino = self.next_available_inode();
    //     let dir = File {
    //         name: name.to_str().unwrap().to_string(),
    //         attr: FileAttr {
    //             ino: ino,
    //             kind: FileType::Directory,
    //             size: 0,
    //             blocks: 123,
    //             atime: Timespec::new(1, 0),
    //             mtime: Timespec::new(1, 0),
    //             ctime: Timespec::new(1, 0),
    //             crtime: Timespec::new(1, 0),
    //             perm: 0o644,
    //             nlink: 0,
    //             uid: 0,
    //             gid: 0,
    //             rdev: 0,
    //             flags: 0,
    //         },
    //         drive_file: None,
    //     };

    //     reply.entry(&TTL, &dir.attr, 0);

    //     let parent_id = self.get_node_id(parent).unwrap().clone();
    //     let dir_id: &NodeId = &self.tree
    //         .insert(Node::new(ino), UnderNode(&parent_id))
    //         .unwrap();
    //     self.files.insert(ino, dir);
    //     self.node_ids.insert(ino, dir_id.clone());
    // }

    // fn rmdir(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
    //     let ino = self.get_file_from_parent(parent, name).unwrap().inode();
    //     let id = self.get_node_id(ino).unwrap().clone();

    //     if self.tree.children(&id).unwrap().next().is_some() {
    //         reply.error(ENOTEMPTY);
    //         return;
    //     }

    //     match self.remove(ino) {
    //         Ok(()) => reply.ok(),
    //         Err(_e) => reply.error(ENOENT),
    //     };
    // }

    // fn flush(&mut self, _req: &Request, ino: Inode, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
    //     let file = self.get_file(ino).unwrap().drive_file.as_ref();
    //     // TODO: uncomment this
    //     // self.drive_fetcher.flush(ino);
    //     reply.ok();
    // }

    // fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
    //     if !self.statfs_cache.contains_key("size") || !self.statfs_cache.contains_key("capacity") {
    //         let (size, capacity) = self.drive_fetcher.size_and_capacity();
    //         let capacity = capacity.unwrap_or(std::i64::MAX as u64);
    //         self.statfs_cache.insert("size".to_string(), size);
    //         self.statfs_cache.insert("capacity".to_string(), capacity);
    //     }

    //     let size = self.statfs_cache.get("size").unwrap().to_owned();
    //     let capacity = self.statfs_cache.get("capacity").unwrap().to_owned();

    //     reply.statfs(
    //         /* blocks:*/ capacity,
    //         /* bfree: */ capacity - size,
    //         /* bavail: */ capacity - size,
    //         /* files: */ std::u64::MAX,
    //         /* ffree: */ std::u64::MAX - self.files.len() as u64,
    //         /* bsize: */ 1,
    //         /* namelen: */ 1024,
    //         /* frsize: */ 1,
    //     );
    // }
}
