use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyStatfs, ReplyWrite, Request};
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use id_tree::{Node, NodeId, NodeIdError, Tree, TreeBuilder};
use libc::{ENOENT, ENOTDIR, ENOTEMPTY};
use std::clone::Clone;
use std::cmp;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std;
use time::Timespec;

use super::File;
use super::fetcher::DataFetcher;

pub type Inode = u64;

pub struct GCSF<DF: DataFetcher> {
    tree: Tree<Inode>,
    files: HashMap<Inode, File>,
    ids: HashMap<Inode, NodeId>,
    data_fetcher: DF,
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 }; // 1 second

impl<DF: DataFetcher> GCSF<DF> {
    pub fn new() -> GCSF<DF> {
        let mut tree = TreeBuilder::new().with_node_capacity(1).build();

        let root = File {
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
        };

        let root_inode = root.inode();
        let root_id: NodeId = tree.insert(Node::new(root.inode()), AsRoot).unwrap();

        GCSF {
            tree,
            files: hashmap!{root_inode => root},
            ids: hashmap!{root_inode => root_id},
            data_fetcher: DF::new(),
        }
    }

    fn get_file_from_parent(&self, parent: Inode, name: &OsStr) -> Option<&File> {
        let name = name.to_str().unwrap();
        self.tree
            .children(self.get_node_id(parent)?)
            .unwrap()
            .map(|child| self.get_file(*child.data()).unwrap())
            .find(|file| file.name == name)
    }

    fn get_file_with_id(&self, id: &NodeId) -> Option<&File> {
        match self.tree.get(id) {
            Ok(node) => self.files.get(node.data()),
            Err(_e) => None,
        }
    }

    fn get_file(&self, ino: Inode) -> Option<&File> {
        self.files.get(&ino)
    }

    fn get_mut_file(&mut self, ino: Inode) -> Option<&mut File> {
        self.files.get_mut(&ino)
    }

    #[allow(dead_code)]
    fn get_node(&self, ino: Inode) -> Result<&Node<Inode>, NodeIdError> {
        let node_id = self.ids.get(&ino).unwrap();
        self.tree.get(&node_id)
    }

    fn get_node_id(&self, ino: Inode) -> Option<&NodeId> {
        self.ids.get(&ino)
    }

    fn contains(&self, ino: &Inode) -> bool {
        self.files.contains_key(&ino)
    }

    fn remove(&mut self, ino: Inode) -> Result<(), &str> {
        let id = self.get_node_id(ino).ok_or("NodeId not found")?.clone();
        let _result = self.tree.remove_node(id, DropChildren);
        self.files.remove(&ino);
        self.ids.remove(&ino);
        self.data_fetcher.remove(ino);

        Ok(())
    }

    fn next_available_inode(&self) -> Inode {
        (1..)
            .filter(|inode| !self.contains(inode))
            .take(1)
            .next()
            .unwrap()
    }
}

impl<DF: DataFetcher> Filesystem for GCSF<DF> {
    fn lookup(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEntry) {
        debug!("{:?}", self);
        match self.get_file_from_parent(parent, name) {
            Some(ref file) => {
                reply.entry(&TTL, &file.attr, 0);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn getattr(&mut self, _req: &Request, ino: Inode, reply: ReplyAttr) {
        match self.get_file(ino) {
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
        reply.data(
            self.data_fetcher
                .read(ino, offset as usize, size as usize)
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
        self.data_fetcher.write(ino, offset, data);

        match self.get_mut_file(ino) {
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
        match self.get_node_id(ino) {
            Some(wd_id) => {
                let mut curr_offs = offset + 1;

                // https://github.com/libfuse/libfuse/blob/master/include/fuse_lowlevel.h#L693
                // reply.add(ino, curr_offs, wd_file.attr.kind, ".");
                // curr_offs += 1;

                for child in self.tree.children(wd_id).unwrap().skip(offset as usize) {
                    let file = self.get_file(*child.data()).unwrap();
                    reply.add(file.inode(), curr_offs, file.kind(), &file.name);
                    curr_offs += 1;
                }
                reply.ok();
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn rename(
        &mut self,
        _req: &Request,
        parent: Inode,
        name: &OsStr,
        new_parent: u64,
        new_name: &OsStr,
        reply: ReplyEmpty,
    ) {
        let file_inode = self.get_file_from_parent(parent, name).unwrap().inode();

        // TODO: is to_owned() safe?
        let file_id = self.get_node_id(file_inode).unwrap().to_owned();
        let new_parent_id = self.get_node_id(new_parent).unwrap().to_owned();

        let _result = self.tree.move_node(&file_id, ToParent(&new_parent_id));
        self.get_mut_file(file_inode).unwrap().name = new_name.to_str().unwrap().to_string();

        reply.ok()
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
        if !self.files.contains_key(&ino) {
            error!("create: could not find inode={} in the file tree", ino);
            reply.error(ENOENT);
            return;
        }

        let file = self.get_mut_file(ino).unwrap();

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
        if !self.files.contains_key(&parent) {
            error!(
                "create: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }

        let ino = self.next_available_inode();
        let file = File {
            name: name.to_str().unwrap().to_string(),
            attr: FileAttr {
                ino: ino,
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
        };

        reply.created(&TTL, &file.attr, 0, 0, 0);

        let parent_id = self.get_node_id(parent).unwrap().clone();
        let file_id: &NodeId = &self.tree
            .insert(Node::new(ino), UnderNode(&parent_id))
            .unwrap();
        self.files.insert(ino, file);
        self.ids.insert(ino, file_id.clone());
    }

    fn unlink(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        let ino = self.get_file_from_parent(parent, name).unwrap().inode();

        match self.remove(ino) {
            Ok(()) => reply.ok(),
            Err(_e) => reply.error(ENOENT),
        };
    }

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: Inode,
        name: &OsStr,
        _mode: u32,
        reply: ReplyEntry,
    ) {
        if !self.files.contains_key(&parent) {
            error!(
                "mkdir: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }

        let ino = self.next_available_inode();
        let dir = File {
            name: name.to_str().unwrap().to_string(),
            attr: FileAttr {
                ino: ino,
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
        };

        reply.entry(&TTL, &dir.attr, 0);

        let parent_id = self.get_node_id(parent).unwrap().clone();
        let dir_id: &NodeId = &self.tree
            .insert(Node::new(ino), UnderNode(&parent_id))
            .unwrap();
        self.files.insert(ino, dir);
        self.ids.insert(ino, dir_id.clone());
    }

    fn rmdir(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        let ino = self.get_file_from_parent(parent, name).unwrap().inode();
        let id = self.get_node_id(ino).unwrap().clone();

        if self.tree.children(&id).unwrap().next().is_some() {
            reply.error(ENOTEMPTY);
            return;
        }

        match self.remove(ino) {
            Ok(()) => reply.ok(),
            Err(_e) => reply.error(ENOENT),
        };
    }

    fn flush(&mut self, _req: &Request, ino: Inode, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        self.data_fetcher.flush(ino);
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        let (size, capacity) = self.data_fetcher.size_and_capacity();
        let capacity = capacity.unwrap_or(std::i64::MAX as u64);

        reply.statfs(
            /* blocks:*/ capacity,
            /* bfree: */ capacity - size,
            /* bavail: */ capacity - size,
            /* files: */ std::u64::MAX,
            /* ffree: */ std::u64::MAX - self.files.len() as u64,
            /* bsize: */ 1,
            /* namelen: */ 1024,
            /* frsize: */ 1,
        );
    }
}

impl<DF: DataFetcher> fmt::Debug for GCSF<DF> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GCSF(\n")?;

        if self.tree.root_node_id().is_none() {
            return write!(f, ")\n");
        }

        let mut stack: Vec<(u32, &NodeId)> = vec![(0, self.tree.root_node_id().unwrap())];

        while !stack.is_empty() {
            let (level, node_id) = stack.pop().unwrap();

            for _ in 0..level {
                write!(f, "\t")?;
            }

            let file = self.get_file_with_id(node_id).unwrap();
            // let preview_string = str::from_utf8(
            //     self.data_fetcher.read(file.inode(), 0, 100).unwrap_or(&[]),
            // ).unwrap_or("binary file");

            write!(f, "{:3} => {}\n", file.inode(), file.name)?;

            self.tree.children_ids(node_id).unwrap().for_each(|id| {
                stack.push((level + 1, id));
            });
        }

        write!(f, ")\n")
    }
}
