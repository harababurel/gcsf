use GoogleDriveFetcher;
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
    tree: Tree<Inode>,
    files: HashMap<Inode, super::File>,
    node_ids: HashMap<Inode, NodeId>,
    drive_ids: HashMap<DriveId, Inode>,
    drive_fetcher: GoogleDriveFetcher,

    statfs_cache: LruCache<String, u64>,
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 }; // 1 second

impl GCSF {
    pub fn new() -> GCSF {
        let mut tree = TreeBuilder::new().with_node_capacity(500).build();

        let root = GCSF::new_root_file();
        let root_inode = root.inode();
        let root_id: NodeId = tree.insert(Node::new(root_inode), AsRoot).unwrap();

        let shared = GCSF::new_shared_with_me_file();
        let shared_inode = shared.inode();
        let shared_id: NodeId = tree.insert(Node::new(shared_inode), UnderNode(&root_id))
            .unwrap();

        let mut files = hashmap!{root_inode => root, shared_inode => shared};
        let mut node_ids =
            hashmap!{root_inode => root_id.clone(), shared_inode => shared_id.clone()};
        let mut drive_ids = HashMap::new();
        let mut drive_fetcher = GoogleDriveFetcher::new();

        let mut inode = 3;
        let mut q: LinkedList<String> = LinkedList::new();
        q.push_back(drive_fetcher.root_id());

        while !q.is_empty() {
            let parent_id = q.pop_front().unwrap();

            for drive_file in drive_fetcher.get_all_files(Some(&parent_id)) {
                let id = drive_file.id.clone().unwrap();

                drive_ids.insert(id.clone(), inode);

                let file = super::File::from_drive_file(inode, drive_file);
                if file.kind() == FileType::Directory {
                    q.push_back(id);
                }
                files.insert(inode, file);

                let parent_inode = match drive_ids.get(&parent_id) {
                    Some(inode) => *inode,
                    _ => 1,
                };
                let parent_node_id = node_ids.get(&parent_inode).unwrap().clone();
                let node_id: NodeId = tree.insert(Node::new(inode), UnderNode(&parent_node_id))
                    .unwrap();
                node_ids.insert(inode, node_id);

                inode += 1;
            }
        }

        // Reshape the file tree
        // for file in files.values() {
        //     if file.drive_file.is_none() {
        //         debug!("Found gcsf::File with no drive3::File. Skipping");
        //         continue;
        //     }

        //     let parents = file.drive_file.clone().unwrap().parents;

        //     let mut parent_inode: Inode = 1;
        //     if parents.is_some() {
        //         let parents = parents.unwrap();
        //         // debug!("Finding inode for parent id = {}", &parents[0]);
        //         parent_inode = match drive_ids.get(&parents[0]) {
        //             Some(inode) => *inode,
        //             _ => 2,
        //         };
        //     }

        // debug!(
        //     "Found gcsf::File with no parents: {:?}. Adding to Shared with me dir",
        //     file.drive_file.as_ref().unwrap().name
        // );

        // let parent_node_id = node_ids.get(&parent_inode).unwrap();
        // let file_node_id = node_ids.get(&file.inode()).unwrap();
        // tree.move_node(file_node_id, ToParent(parent_node_id));
        // }

        GCSF {
            tree,
            files,
            node_ids,
            drive_ids,
            drive_fetcher,
            statfs_cache: LruCache::<String, u64>::with_expiry_duration_and_capacity(
                std::time::Duration::from_secs(5),
                2,
            ),
        }
    }

    fn new_root_file() -> super::File {
        super::File {
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
            drive_file: None,
        }
    }

    fn new_shared_with_me_file() -> super::File {
        super::File {
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

    fn get_file_from_parent(&self, parent: Inode, name: &OsStr) -> Option<&super::File> {
        let name = name.to_str().unwrap();
        self.tree
            .children(self.get_node_id(parent)?)
            .unwrap()
            .map(|child| self.get_file(*child.data()).unwrap())
            .find(|file| file.name == name)
    }

    fn get_file_with_id(&self, id: &NodeId) -> Option<&super::File> {
        match self.tree.get(id) {
            Ok(node) => self.files.get(node.data()),
            Err(_e) => None,
        }
    }

    fn get_file(&self, ino: Inode) -> Option<&super::File> {
        self.files.get(&ino)
    }

    fn get_mut_file(&mut self, ino: Inode) -> Option<&mut super::File> {
        self.files.get_mut(&ino)
    }

    #[allow(dead_code)]
    fn get_node(&self, ino: Inode) -> Result<&Node<Inode>, NodeIdError> {
        let node_id = self.node_ids.get(&ino).unwrap();
        self.tree.get(&node_id)
    }

    fn get_node_id(&self, ino: Inode) -> Option<&NodeId> {
        self.node_ids.get(&ino)
    }

    fn contains(&self, ino: &Inode) -> bool {
        self.files.contains_key(&ino)
    }

    fn remove(&mut self, ino: Inode) -> Result<(), &str> {
        let id = self.get_node_id(ino).ok_or("NodeId not found")?.clone();
        let _result = self.tree.remove_node(id, DropChildren);
        self.files.remove(&ino);
        self.node_ids.remove(&ino);
        self.drive_fetcher.remove(ino);

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

impl Filesystem for GCSF {
    fn lookup(&mut self, _req: &Request, parent: Inode, name: &OsStr, reply: ReplyEntry) {
        // debug!("{:?}", self);
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
        let drive_id = self.get_file(ino).unwrap().drive_id().unwrap();

        reply.data(
            self.drive_fetcher
                .read(&drive_id, offset as usize, size as usize)
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
        self.drive_fetcher.write(ino, offset, data);

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
        let file = super::File {
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
            drive_file: None,
        };

        reply.created(&TTL, &file.attr, 0, 0, 0);

        let parent_id = self.get_node_id(parent).unwrap().clone();
        let file_id: &NodeId = &self.tree
            .insert(Node::new(ino), UnderNode(&parent_id))
            .unwrap();
        self.files.insert(ino, file);
        self.node_ids.insert(ino, file_id.clone());
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
        let dir = super::File {
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
            drive_file: None,
        };

        reply.entry(&TTL, &dir.attr, 0);

        let parent_id = self.get_node_id(parent).unwrap().clone();
        let dir_id: &NodeId = &self.tree
            .insert(Node::new(ino), UnderNode(&parent_id))
            .unwrap();
        self.files.insert(ino, dir);
        self.node_ids.insert(ino, dir_id.clone());
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
        //TODO: uncomment this
        // self.drive_fetcher.flush(ino);
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        if !self.statfs_cache.contains_key("size") || !self.statfs_cache.contains_key("capacity") {
            let (size, capacity) = self.drive_fetcher.size_and_capacity();
            let capacity = capacity.unwrap_or(std::i64::MAX as u64);
            self.statfs_cache.insert("size".to_string(), size);
            self.statfs_cache.insert("capacity".to_string(), capacity);
        }

        let size = self.statfs_cache.get("size").unwrap().to_owned();
        let capacity = self.statfs_cache.get("capacity").unwrap().to_owned();

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

impl fmt::Debug for GCSF {
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
            //     self.drive_fetcher.read(file.inode(), 0, 100).unwrap_or(&[]),
            // ).unwrap_or("binary file");

            write!(f, "{:3} => {}\n", file.inode(), file.name)?;

            self.tree.children_ids(node_id).unwrap().for_each(|id| {
                stack.push((level + 1, id));
            });
        }

        write!(f, ")\n")
    }
}
