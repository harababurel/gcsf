use drive3;
use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyWrite, Request};
use hyper;
use hyper_rustls;
use id_tree::{Node, NodeId, Tree, TreeBuilder};
use id_tree::InsertBehavior::*;
use id_tree::RemoveBehavior::*;
use libc::{EISDIR, ENOENT, ENOTDIR, ENOTEMPTY};
use oauth2;
use serde_json;
use std::clone::Clone;
use std::cmp;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
// use std::thread;
use time::Timespec;

type GCClient = hyper::Client;
type GCAuthenticator = oauth2::Authenticator<
    oauth2::DefaultAuthenticatorDelegate,
    oauth2::DiskTokenStorage,
    hyper::Client,
>;
type GCDrive = drive3::Drive<GCClient, GCAuthenticator>;

pub struct GCSF {
    drive: GCDrive,
    tree: Tree<File>,
    inode_to_node: HashMap<u64, NodeId>,
}

#[derive(Clone)]
struct File {
    name: String,
    attr: FileAttr,
    pieces: Vec<String>, // filename of each piece of this file on google drive
    data: Option<Vec<u8>>,
}

impl GCSF {
    pub fn new() -> GCSF {
        let wd = File {
            name: String::from("."),
            attr: HELLO_DIR_ATTR,
            pieces: vec![],
            data: None,
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
            pieces: vec![],
            data: None,
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
            pieces: vec![String::from("100.bin")],
            data: Some(String::from("world file here\n").into_bytes()),
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
            pieces: vec![
                String::from("1.bin"),
                String::from("2.bin"),
                String::from("3.bin"),
            ],
            data: Some(String::from("some file content\n").into_bytes()),
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
            pieces: vec![String::from("123.bin"), String::from("456.bin")],
            data: Some(String::from("other file content\n").into_bytes()),
        };

        let mut tree: Tree<File> = TreeBuilder::new().with_node_capacity(10).build();

        let wd_id: NodeId = tree.insert(Node::new(wd.clone()), AsRoot).unwrap();
        let some_file_id = tree.insert(Node::new(some_file.clone()), UnderNode(&wd_id))
            .unwrap();
        let other_file_id = tree.insert(Node::new(other_file.clone()), UnderNode(&wd_id))
            .unwrap();
        let hello_dir_id = tree.insert(Node::new(hello_dir.clone()), UnderNode(&wd_id))
            .unwrap();
        let world_file_id = tree.insert(Node::new(world_file.clone()), UnderNode(&hello_dir_id))
            .unwrap();

        let inode_to_node = hashmap!{
            wd.attr.ino => wd_id,
            some_file.attr.ino => some_file_id,
            other_file.attr.ino => other_file_id,
            hello_dir.attr.ino => hello_dir_id,
            world_file.attr.ino => world_file_id,
        };

        GCSF {
            drive: GCSF::create_drive(),
            tree,
            inode_to_node,
        }
    }

    fn get_file(&self, ino: u64) -> Option<&File> {
        match self.inode_to_node.get(&ino) {
            Some(node_id) => Some(self.tree.get(node_id).unwrap().data()),
            None => None,
        }
    }

    fn get_mut_file(&mut self, ino: u64) -> Option<&mut File> {
        match self.inode_to_node.get(&ino) {
            Some(node_id) => Some(self.tree.get_mut(node_id).unwrap().data_mut()),
            None => None,
        }
    }

    fn get_node(&self, ino: u64) -> Option<&Node<File>> {
        match self.inode_to_node.get(&ino) {
            Some(node_id) => Some(self.tree.get(node_id).unwrap()),
            None => None,
        }
    }

    fn get_node_id(&self, ino: u64) -> Option<&NodeId> {
        self.inode_to_node.get(&ino)
    }

    fn next_available_inode(&self) -> u64 {
        (1..)
            .filter(|inode| !self.inode_to_node.contains_key(inode))
            .take(1)
            .next()
            .unwrap()
    }

    fn read_client_secret(file: &str) -> oauth2::ApplicationSecret {
        use std::fs::OpenOptions;
        use std::io::Read;

        match OpenOptions::new().read(true).open(file) {
            Ok(mut f) => {
                let mut secret = String::new();
                f.read_to_string(&mut secret);

                let app_secret: oauth2::ConsoleApplicationSecret =
                    serde_json::from_str(secret.as_str()).unwrap();

                app_secret.installed.unwrap()
            }
            Err(e) => {
                error!("Could not read client secret: {}", e);
                panic!();
            }
        }
    }

    fn create_drive_auth() -> GCAuthenticator {
        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        //
        let secret: oauth2::ApplicationSecret = GCSF::read_client_secret("client_secret.json");

        // Instantiate the authenticator. It will choose a suitable authentication flow for you,
        // unless you replace  `None` with the desired Flow.
        // Provide your own `AuthenticatorDelegate` to adjust the way it operates
        // and get feedback about
        // what's going on. You probably want to bring in your own `TokenStorage`
        // to persist tokens and
        // retrieve them from storage.
        let auth = oauth2::Authenticator::new(
            &secret,
            oauth2::DefaultAuthenticatorDelegate,
            hyper::Client::with_connector(hyper::net::HttpsConnector::new(
                hyper_rustls::TlsClient::new(),
            )),
            // <MemoryStorage as Default>::default(),
            oauth2::DiskTokenStorage::new(&String::from("/tmp/gcsf_token.json")).unwrap(),
            Some(oauth2::FlowType::InstalledRedirect(8080)), // This is the main change!
        );

        auth
    }

    fn create_drive() -> GCDrive {
        let auth = GCSF::create_drive_auth();
        drive3::Drive::new(
            hyper::Client::with_connector(hyper::net::HttpsConnector::new(
                hyper_rustls::TlsClient::new(),
            )),
            auth,
        )
    }

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

impl Filesystem for GCSF {
    fn lookup(&mut self, _req: &Request, parent_ino: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("{:#?}", &self);

        match self.get_node(parent_ino) {
            Some(node) => {
                for child_id in node.children() {
                    let child_node = self.tree.get(child_id).unwrap();
                    let file = child_node.data();
                    if file.name == name.to_str().unwrap() {
                        reply.entry(&TTL, &file.attr, 0);
                        return;
                    }
                }

                reply.error(ENOENT);
            }
            None => {
                reply.error(ENOENT);
            }
        };
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
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
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        match self.get_file(ino) {
            Some(ref file) => {
                if file.data.is_some() {
                    let data = file.data.to_owned().unwrap();
                    reply.data(
                        &data[offset as usize
                                  ..cmp::min(data.len(), offset as usize + size as usize)],
                    );
                } else {
                    reply.error(ENOENT);
                }
            }
            None => {
                error!("read: could not find ino={}", ino);
            }
        };
    }

    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _flags: u32,
        reply: ReplyWrite,
    ) {
        let offset: usize = cmp::max(offset, 0) as usize;

        match self.get_mut_file(ino) {
            Some(ref mut file) => {
                if !file.data.is_some() {
                    file.data = Some(Vec::new());
                }

                let new_data: &mut Vec<u8> = file.data.as_mut().unwrap();

                // TODO: resize might not be the best choice, because it truncates if the new size is small
                let old_size = new_data.len();
                let new_size = offset + data.len();
                new_data.resize(new_size, 0);

                if new_size < old_size {
                    new_data.shrink_to_fit();
                }

                // TODO: memcpy or similar
                for i in offset..offset + data.len() {
                    new_data[i] = data[i - offset];
                }

                file.attr.size = new_data.len() as u64;
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
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        match self.get_node(ino) {
            Some(node) => {
                if offset == 0 {
                    let wd_id = self.get_node_id(ino).unwrap();
                    let wd_node = self.tree.get(wd_id).unwrap();
                    let wd_file = wd_node.data();

                    let mut curr_offs = 1;
                    reply.add(wd_file.attr.ino, curr_offs, wd_file.attr.kind, ".");
                    curr_offs += 1;

                    if wd_node.parent().is_some() {
                        let parent_node = self.tree.get(wd_node.parent().unwrap()).unwrap();
                        let parent_file = parent_node.data();
                        reply.add(parent_file.attr.ino, curr_offs, parent_file.attr.kind, "..");
                        curr_offs += 1;
                    }

                    for child_id in wd_node.children() {
                        let child_node = self.tree.get(child_id).unwrap();
                        let file = child_node.data();
                        reply.add(file.attr.ino, curr_offs, file.attr.kind, &file.name);
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

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
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
        if !self.inode_to_node.contains_key(&ino) {
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
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _flags: u32,
        reply: ReplyCreate,
    ) {
        if !self.inode_to_node.contains_key(&parent) {
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
            pieces: vec![],
            data: None,
        };

        reply.created(&TTL, &file.attr, 0, 0, 0);

        let parent_id = self.inode_to_node.get(&parent).unwrap().clone();
        let file_id: &NodeId = &self.tree
            .insert(Node::new(file), UnderNode(&parent_id))
            .unwrap();

        self.inode_to_node.insert(ino, file_id.clone());
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        if !self.inode_to_node.contains_key(&parent) {
            error!(
                "unlink: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }

        match self.get_node(parent)
            .unwrap()
            .children()
            .into_iter()
            .find(|id| self.tree.get(id).unwrap().data().name == name.to_str().unwrap())
            .map_or(None, |id| Some(id.clone()))
        {
            Some(id) => {
                if self.tree.get(&id).unwrap().data().attr.kind == FileType::Directory {
                    reply.error(EISDIR);
                    return;
                }
                let ino = self.tree.get(&id).unwrap().data().attr.ino;

                self.inode_to_node.remove(&ino);
                self.tree.remove_node(id, DropChildren); // can't even have children

                reply.ok();
            }
            None => {
                reply.error(ENOTDIR);
            }
        };
    }

    fn mkdir(&mut self, _req: &Request, parent: u64, name: &OsStr, _mode: u32, reply: ReplyEntry) {
        if !self.inode_to_node.contains_key(&parent) {
            error!(
                "mkdir: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }

        let ino = self.next_available_inode();
        let child_dir = File {
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
            pieces: vec![],
            data: None,
        };

        reply.entry(&TTL, &child_dir.attr, 0);

        let parent_id = self.inode_to_node.get(&parent).unwrap().clone();
        let child_id: &NodeId = &self.tree
            .insert(Node::new(child_dir), UnderNode(&parent_id))
            .unwrap();

        self.inode_to_node.insert(ino, child_id.clone());
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        if !self.inode_to_node.contains_key(&parent) {
            error!(
                "rmdir: could not find parent inode={} in the file tree",
                parent
            );
            reply.error(ENOTDIR);
            return;
        }

        match self.get_node(parent)
            .unwrap()
            .children()
            .into_iter()
            .find(|id| self.tree.get(id).unwrap().data().name == name.to_str().unwrap())
            .map_or(None, |id| Some(id.clone()))
        {
            Some(id) => {
                if !self.tree.get(&id).unwrap().children().is_empty() {
                    reply.error(ENOTEMPTY);
                    return;
                }

                let ino = self.tree.get(&id).unwrap().data().attr.ino;
                self.inode_to_node.remove(&ino);
                self.tree.remove_node(id, DropChildren);

                reply.ok();
            }
            None => {
                reply.error(ENOTDIR);
            }
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

impl fmt::Debug for GCSF {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::str;
        write!(f, "GCSF(\n");

        let mut stack: Vec<(u32, &NodeId)> = vec![(0, self.tree.root_node_id().unwrap())];

        while !stack.is_empty() {
            let (level, node_id) = stack.pop().unwrap();
            let node = self.tree.get(node_id).unwrap();

            (0..level).for_each(|_| {
                write!(f, "\t");
            });

            let file = node.data();
            write!(f, "{:3} => {}", file.attr.ino, file.name);
            if file.data.is_some() {
                let preview_data: Vec<u8> = file.data
                    .as_ref()
                    .unwrap()
                    .clone()
                    .into_iter()
                    .take(50)
                    .collect();

                let preview_string = str::from_utf8(preview_data.as_slice()).unwrap_or("binary");
                write!(f, " ({:?})", preview_string);
                if preview_data.len() < file.data.as_ref().unwrap().len() {
                    write!(f, "...");
                }
            }
            write!(f, "\n");

            for child_id in node.children() {
                stack.push((level + 1, child_id));
            }
        }

        write!(f, ")\n")
    }
}
