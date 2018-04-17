use drive3;
use fuse;
use fuse::{FileAttr, FileType, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use hyper;
use hyper_rustls;
use id_tree;
use libc;
use oauth2;
use oauth2::{ApplicationSecret, Authenticator, ConsoleApplicationSecret,
             DefaultAuthenticatorDelegate, DiskTokenStorage};
use serde_json;
use std::clone::Clone;
use std::collections::HashMap;
use std::ffi::OsStr;
use time::Timespec;

type GCClient = hyper::Client;
type GCAuthenticator = oauth2::Authenticator<
    oauth2::DefaultAuthenticatorDelegate,
    oauth2::DiskTokenStorage,
    hyper::Client,
>;
type GCDrive = drive3::Drive<GCClient, GCAuthenticator>;

pub struct GCSF {
    hub: GCDrive,
    hierarchy: id_tree::Tree<File>,
    inode_to_node: HashMap<u64, id_tree::NodeId>,
}

#[derive(Clone)]
struct File {
    name: String,
    attr: fuse::FileAttr,
    pieces: Vec<String>, // filename of each piece of this file on google drive
}

impl GCSF {
    pub fn new() -> GCSF {
        let wd = File {
            name: String::from("."),
            attr: HELLO_DIR_ATTR,
            pieces: vec![],
        };

        let hello_dir = File {
            name: String::from("hello"),
            attr: fuse::FileAttr {
                ino: 2,
                kind: fuse::FileType::Directory,
                size: 0,
                blocks: 0,
                atime: Timespec::new(1, 0),
                mtime: Timespec::new(1, 0),
                ctime: Timespec::new(1, 0),
                crtime: Timespec::new(1, 0),
                perm: 0,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            pieces: vec![],
        };

        let world_file = File {
            name: String::from("world.txt"),
            attr: fuse::FileAttr {
                ino: 13,
                kind: fuse::FileType::RegularFile,
                size: 1280,
                blocks: 0,
                atime: Timespec::new(1, 0),
                mtime: Timespec::new(1, 0),
                ctime: Timespec::new(1, 0),
                crtime: Timespec::new(1, 0),
                perm: 0,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            pieces: vec![String::from("100.bin")],
        };

        let some_file = File {
            name: String::from("some_file.txt"),
            attr: fuse::FileAttr {
                ino: 10,
                kind: fuse::FileType::RegularFile,
                size: 0,
                blocks: 0,
                atime: Timespec::new(1, 0),
                mtime: Timespec::new(1, 0),
                ctime: Timespec::new(1, 0),
                crtime: Timespec::new(1, 0),
                perm: 0,
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
        };

        let other_file = File {
            name: String::from("other_file.txt"),
            attr: fuse::FileAttr {
                ino: 11,
                kind: fuse::FileType::RegularFile,
                size: 0,
                blocks: 0,
                atime: Timespec::new(1, 0),
                mtime: Timespec::new(1, 0),
                ctime: Timespec::new(1, 0),
                crtime: Timespec::new(1, 0),
                perm: 0,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            pieces: vec![String::from("123.bin"), String::from("456.bin")],
        };

        let mut hierarchy: id_tree::Tree<File> =
            id_tree::TreeBuilder::new().with_node_capacity(10).build();

        let wd_id: id_tree::NodeId = hierarchy
            .insert(
                id_tree::Node::new(wd.clone()),
                id_tree::InsertBehavior::AsRoot,
            )
            .unwrap();

        let some_file_id = hierarchy
            .insert(
                id_tree::Node::new(some_file.clone()),
                id_tree::InsertBehavior::UnderNode(&wd_id),
            )
            .unwrap();

        let other_file_id = hierarchy
            .insert(
                id_tree::Node::new(other_file.clone()),
                id_tree::InsertBehavior::UnderNode(&wd_id),
            )
            .unwrap();

        let hello_dir_id = hierarchy
            .insert(
                id_tree::Node::new(hello_dir.clone()),
                id_tree::InsertBehavior::UnderNode(&wd_id),
            )
            .unwrap();

        let world_file_id = hierarchy
            .insert(
                id_tree::Node::new(world_file.clone()),
                id_tree::InsertBehavior::UnderNode(&hello_dir_id),
            )
            .unwrap();

        let inode_to_node = hashmap!{
            wd.attr.ino => wd_id,
            some_file.attr.ino => some_file_id,
            other_file.attr.ino => other_file_id,
            hello_dir.attr.ino => hello_dir_id,
            world_file.attr.ino => world_file_id,
        };

        info!("hierarchy has height = {}", hierarchy.height());

        GCSF {
            hub: GCSF::create_drive_hub(),
            hierarchy,
            inode_to_node,
        }
    }

    fn get_file(&self, ino: u64) -> Option<&File> {
        match self.inode_to_node.get(&ino) {
            Some(node_id) => Some(self.hierarchy.get(node_id).unwrap().data()),
            None => None,
        }
    }

    fn get_node(&self, ino: u64) -> Option<&id_tree::Node<File>> {
        match self.inode_to_node.get(&ino) {
            Some(node_id) => Some(self.hierarchy.get(node_id).unwrap()),
            None => None,
        }
    }

    fn read_client_secret(file: &str) -> ApplicationSecret {
        use std::fs::OpenOptions;
        use std::io::Read;

        let mut secret = String::new();
        OpenOptions::new()
            .read(true)
            .open(file)
            .unwrap()
            .read_to_string(&mut secret);
        let consappsec: ConsoleApplicationSecret = serde_json::from_str(secret.as_str()).unwrap();
        consappsec.installed.unwrap()
    }

    fn create_drive_auth() -> GCAuthenticator {
        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        let secret: ApplicationSecret = GCSF::read_client_secret("client_secret.json");

        // Instantiate the authenticator. It will choose a suitable authentication flow for you,
        // unless you replace  `None` with the desired Flow.
        // Provide your own `AuthenticatorDelegate` to adjust the way it operates
        // and get feedback about
        // what's going on. You probably want to bring in your own `TokenStorage`
        // to persist tokens and
        // retrieve them from storage.
        let auth = Authenticator::new(
            &secret,
            DefaultAuthenticatorDelegate,
            hyper::Client::with_connector(hyper::net::HttpsConnector::new(
                hyper_rustls::TlsClient::new(),
            )),
            // <MemoryStorage as Default>::default(),
            DiskTokenStorage::new(&String::from("/tmp/gcsf_token.json")).unwrap(),
            Some(oauth2::FlowType::InstalledRedirect(8080)), // This is the main change!
        );

        auth
    }

    fn create_drive_hub() -> GCDrive {
        let auth = GCSF::create_drive_auth();
        drive3::Drive::new(
            hyper::Client::with_connector(hyper::net::HttpsConnector::new(
                hyper_rustls::TlsClient::new(),
            )),
            auth,
        )
    }

    fn ls(&self) -> Vec<drive3::File> {
        let result = self.hub.files()
        .list()
        .spaces("drive")
        .page_size(10)
        // .order_by("folder,modifiedTime desc,name")
        .corpora("user") // or "domain"
        .doit();

        match result {
            Err(e) => {
                println!("{:#?}", e);
                vec![]
            }
            Ok(res) => res.1.files.unwrap().into_iter().collect(),
        }
    }

    // fn cat(&self, filename: &str) -> String {
    //     let result = self.hub.files()
    //     .list()
    //     .spaces("drive")
    //     .page_size(10)
    //     // .order_by("folder,modifiedTime desc,name")
    //     .corpora("user") // or "domain"
    //     .doit();
    // }
}

impl fuse::Filesystem for GCSF {
    fn lookup(&mut self, _req: &Request, parent_ino: u64, name: &OsStr, reply: ReplyEntry) {
        match self.get_node(parent_ino) {
            Some(node) => for child_id in node.children() {
                let child_node = self.hierarchy.get(child_id).unwrap();
                let file = child_node.data();
                if file.name == name.to_str().unwrap() {
                    reply.entry(&TTL, &file.attr, 0);
                    break;
                }
            },
            None => {
                reply.error(libc::ENOENT);
            }
        };
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        if ino == 1 {
            reply.attr(&TTL, &HELLO_DIR_ATTR);
            return;
        }

        // for (name, file) in &self.files {
        //     if file.attr.ino == ino {
        //         reply.attr(&TTL, &file.attr.clone());
        //         return;
        //     }
        // }

        reply.error(libc::ENOENT);
    }

    // Return contents of file. Not necessary yet.
    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        _size: u32,
        reply: ReplyData,
    ) {
        // if ino == 2 {
        //     reply.data(&HELLO_TXT_CONTENT.as_bytes()[offset as usize..]);
        // } else {
        reply.error(libc::ENOENT);
        // }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        // info!("readdir(); Request:");
        // info!("{:#?}", _req);
        info!("ino: {}", ino);
        info!("_fh: {}", _fh);
        info!("offset: {}", offset);

        match self.get_node(ino) {
            Some(node) => {
                info!("matched node file = {}", node.data().name);
                if offset == 0 {
                    let wd_id = self.inode_to_node.get(&ino).unwrap();
                    let wd_node = self.hierarchy.get(wd_id).unwrap();

                    let mut curr_offs = 1;
                    reply.add(
                        wd_node.data().attr.ino,
                        curr_offs,
                        wd_node.data().attr.kind,
                        ".",
                    );
                    reply.add(2, 1, FileType::Directory, "..");

                    for child_id in wd_node.children() {
                        let child_node = self.hierarchy.get(child_id).unwrap();
                        let file = child_node.data();
                        info!("child name: {}", &file.name);
                        //                  ino  offset               kind        name
                        reply.add(file.attr.ino, curr_offs, file.attr.kind, &file.name);
                        curr_offs += 1;
                    }
                }
                reply.ok();
            }
            None => {
                reply.error(libc::ENOENT);
            }
        }
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
