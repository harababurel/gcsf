use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyWrite, Request};
use id_tree::{Node, NodeId, NodeIdError, Tree, TreeBuilder};
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use libc::{EISDIR, ENOENT, ENOTDIR, ENOTEMPTY};
use std::clone::Clone;
use std::cmp;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use failure::{err_msg, Error, ResultExt};
use time::Timespec;

use super::File;
use super::fetcher::{DataFetcher, GoogleDriveFetcher, InMemoryFetcher};

pub type Inode = u64;

pub struct GCSF<DF: DataFetcher> {
    // drive: GCDrive,
    tree: Tree<Inode>,
    files: HashMap<Inode, File>,
    ids: HashMap<Inode, NodeId>,
    data_fetcher: DF,
}

impl<DF: DataFetcher> GCSF<DF> {
    pub fn new() -> GCSF<DF> {
        let wd = File {
            name: String::from("."),
            attr: HELLO_DIR_ATTR,
        };

        let hello_dir = File {
            name: String::from("hello"),
            attr: FileAttr {
                ino: 2,
                kind: FileType::Directory,
                size: 0,
                blocks: 0,
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

        let world_file = File {
            name: String::from("world.txt"),
            attr: FileAttr {
                ino: 13,
                kind: FileType::RegularFile,
                size: 1280,
                blocks: 0,
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

        let some_file = File {
            name: String::from("some_file.txt"),
            attr: FileAttr {
                ino: 10,
                kind: FileType::RegularFile,
                size: 1000,
                blocks: 0,
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

        let other_file = File {
            name: String::from("other_file.txt"),
            attr: FileAttr {
                ino: 11,
                kind: FileType::RegularFile,
                size: 1000,
                blocks: 0,
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

        let mut tree: Tree<Inode> = TreeBuilder::new().with_node_capacity(5).build();

        let wd_id: NodeId = tree.insert(Node::new(wd.inode()), AsRoot).unwrap();
        let some_file_id = tree.insert(Node::new(some_file.inode()), UnderNode(&wd_id))
            .unwrap();
        let other_file_id = tree.insert(Node::new(other_file.inode()), UnderNode(&wd_id))
            .unwrap();
        let hello_dir_id = tree.insert(Node::new(hello_dir.inode()), UnderNode(&wd_id))
            .unwrap();
        let world_file_id = tree.insert(Node::new(world_file.inode()), UnderNode(&hello_dir_id))
            .unwrap();

        let ids = hashmap!{
            wd.inode() => wd_id,
            some_file.inode() => some_file_id,
            other_file.inode() => other_file_id,
            hello_dir.inode() => hello_dir_id,
            world_file.inode() => world_file_id,
        };

        let files = hashmap!{
            wd.inode() => wd,
            some_file.inode() => some_file,
            other_file.inode() => other_file,
            hello_dir.inode() => hello_dir,
            world_file.inode() => world_file,
        };

        GCSF {
            // drive: GCSF::create_drive().unwrap(),
            tree,
            files,
            ids,
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
            Err(e) => None,
        }
    }

    fn get_file(&self, ino: Inode) -> Option<&File> {
        self.files.get(&ino)
    }
    fn get_mut_file(&mut self, ino: Inode) -> Option<&mut File> {
        self.files.get_mut(&ino)
    }

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
        self.tree.remove_node(id, DropChildren);
        self.files.remove(&ino);
        self.ids.remove(&ino);

        Ok(())
    }

    fn next_available_inode(&self) -> Inode {
        (1..)
            .filter(|inode| !self.contains(inode))
            .take(1)
            .next()
            .unwrap()
    }

    // fn read_client_secret(file: &str) -> Result<oauth2::ApplicationSecret, Error> {
    //     use std::fs::OpenOptions;
    //     use std::io::Read;

    //     let mut file = OpenOptions::new().read(true).open(file)?;

    //     let mut secret = String::new();
    //     file.read_to_string(&mut secret);

    //     let app_secret: oauth2::ConsoleApplicationSecret = serde_json::from_str(secret.as_str())?;
    //     app_secret
    //         .installed
    //         .ok_or(err_msg("Option did not contain a value."))
    // }

    //fn create_drive_auth() -> Result<GCAuthenticator, Error> {
    //    // Get an ApplicationSecret instance by some means. It contains the `client_id` and
    //    // `client_secret`, among other things.
    //    //
    //    let secret: oauth2::ApplicationSecret = GCSF::read_client_secret("client_secret.json")?;

    //    // Instantiate the authenticator. It will choose a suitable authentication flow for you,
    //    // unless you replace  `None` with the desired Flow.
    //    // Provide your own `AuthenticatorDelegate` to adjust the way it operates
    //    // and get feedback about
    //    // what's going on. You probably want to bring in your own `TokenStorage`
    //    // to persist tokens and
    //    // retrieve them from storage.
    //    let auth = oauth2::Authenticator::new(
    //        &secret,
    //        oauth2::DefaultAuthenticatorDelegate,
    //        hyper::Client::with_connector(hyper::net::HttpsConnector::new(
    //            hyper_rustls::TlsClient::new(),
    //        )),
    //        // <MemoryStorage as Default>::default(),
    //        oauth2::DiskTokenStorage::new(&String::from("/tmp/gcsf_token.json")).unwrap(),
    //        Some(oauth2::FlowType::InstalledRedirect(8080)), // This is the main change!
    //    );

    //    Ok(auth)
    //}

    // fn create_drive() -> Result<GCDrive, Error> {
    //     let auth = GCSF::create_drive_auth()?;
    //     Ok(drive3::Drive::new(
    //         hyper::Client::with_connector(hyper::net::HttpsConnector::new(
    //             hyper_rustls::TlsClient::new(),
    //         )),
    //         auth,
    //     ))
    // }

    // fn ls(&self) -> Vec<drive3::File> {
    //     let result = self.drive.files()
    //     .list()
    //     .spaces("drive")
    //     .page_size(10)
    //     // .order_by("folder,modifiedTime desc,name")
    //     .corpora("user") // or "domain"
    //     .doit();

    //     match result {
    //         Err(e) => {
    //             println!("{:#?}", e);
    //             vec![]
    //         }
    //         Ok(res) => res.1.files.unwrap().into_iter().collect(),
    //     }
    // }

    // fn cat(&self, filename: &str) -> String {
    //     let result = self.drive.files()
    //     .list()
    //     .spaces("drive")
    //     .page_size(10)
    //     // .order_by("folder,modifiedTime desc,name")
    //     .corpora("user") // or "domain"
    //     .doit();
    // }
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
                if offset == 0 {
                    let mut curr_offs = 1;
                    let wd_file = self.get_file(ino).unwrap();
                    reply.add(ino, curr_offs, wd_file.attr.kind, ".");
                    curr_offs += 1;

                    let wd_node = self.tree.get(wd_id).unwrap();
                    if wd_node.parent().is_some() {
                        let parent_node = self.tree.get(wd_node.parent().unwrap()).unwrap();
                        let parent_file = self.get_file(*parent_node.data()).unwrap();
                        reply.add(parent_file.inode(), curr_offs, parent_file.kind(), "..");
                        curr_offs += 1;
                    }

                    for child in self.tree.children(wd_id).unwrap() {
                        let file = self.get_file(*child.data()).unwrap();
                        reply.add(file.inode(), curr_offs, file.kind(), &file.name);
                        curr_offs += 1;
                    }
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

        self.tree.move_node(&file_id, ToParent(&new_parent_id));

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
        _req: &Request,
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
                blocks: 0,
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
            Err(e) => reply.error(ENOENT),
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
                blocks: 0,
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
            Err(e) => reply.error(ENOENT),
        };
    }
}

const CREATE_TIME: Timespec = Timespec {
    sec: 1381237736,
    nsec: 0,
}; // 2013-10-08 08:56
const HELLO_DIR_ATTR: FileAttr = FileAttr {
    ino: 1,
    size: 0,
    blocks: 0,
    atime: CREATE_TIME,
    mtime: CREATE_TIME,
    ctime: CREATE_TIME,
    crtime: CREATE_TIME,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 2,
    uid: 501,
    gid: 20,
    rdev: 0,
    flags: 0,
};
const HELLO_TXT_CONTENT: &'static str = "Hello World!\n";
const HELLO_TXT_ATTR: FileAttr = FileAttr {
    ino: 10,
    size: 128,
    blocks: 1,
    atime: CREATE_TIME,
    mtime: CREATE_TIME,
    ctime: CREATE_TIME,
    crtime: CREATE_TIME,
    kind: FileType::RegularFile,
    perm: 0o644,
    nlink: 1,
    uid: 1000,
    gid: 20,
    rdev: 0,
    flags: 0,
};
const TTL: Timespec = Timespec { sec: 1, nsec: 0 }; // 1 second

impl<DF: DataFetcher> fmt::Debug for GCSF<DF> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::str;
        write!(f, "GCSF(\n");

        let mut stack: Vec<(u32, &NodeId)> = vec![(0, self.tree.root_node_id().unwrap())];

        while !stack.is_empty() {
            let (level, node_id) = stack.pop().unwrap();

            (0..level).for_each(|_| {
                write!(f, "\t");
            });

            let file = self.get_file_with_id(node_id).unwrap();
            write!(f, "{:3} => {}", file.inode(), file.name);
            // if file.data.is_some() {
            //     let preview_data: Vec<u8> = file.data
            //         .as_ref()
            //         .unwrap()
            //         .clone()
            //         .into_iter()
            //         .take(50)
            //         .collect();

            //     let preview_string = str::from_utf8(preview_data.as_slice()).unwrap_or("binary");
            //     write!(f, " ({:?})", preview_string);
            //     if preview_data.len() < file.data.as_ref().unwrap().len() {
            //         write!(f, "...");
            //     }
            // }
            write!(f, "\n");

            self.tree.children_ids(node_id).unwrap().for_each(|id| {
                stack.push((level + 1, id));
            });
        }

        write!(f, ")\n")
    }
}
