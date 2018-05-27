use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyWrite, Request};
use gcsf::File;
use gcsf::filesystem::GCSF;
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use id_tree::{Node, NodeId, NodeIdError, Tree, TreeBuilder};
use time::Timespec;

type Inode = u64;

#[test]
fn gcsf_in_memory() {
    // let gcsf: GCSF<InMemoryFetcher> = GCSF::new();

    // let wd = File {
    //     name: String::from("."),
    //     attr: HELLO_DIR_ATTR,
    // };

    // // TODO: use the hardcoded files in a test

    // let hello_dir = File {
    //     name: String::from("hello"),
    //     attr: FileAttr {
    //         ino: 2,
    //         kind: FileType::Directory,
    //         size: 0,
    //         blocks: 0,
    //         atime: Timespec::new(1, 0),
    //         mtime: Timespec::new(1, 0),
    //         ctime: Timespec::new(1, 0),
    //         crtime: Timespec::new(1, 0),
    //         perm: 0o644,
    //         nlink: 0,
    //         uid: 0,
    //         gid: 0,
    //         rdev: 0,
    //         flags: 0,
    //     },
    // };

    // let world_file = File {
    //     name: String::from("world.txt"),
    //     attr: FileAttr {
    //         ino: 13,
    //         kind: FileType::RegularFile,
    //         size: 1280,
    //         blocks: 0,
    //         atime: Timespec::new(1, 0),
    //         mtime: Timespec::new(1, 0),
    //         ctime: Timespec::new(1, 0),
    //         crtime: Timespec::new(1, 0),
    //         perm: 0o644,
    //         nlink: 0,
    //         uid: 0,
    //         gid: 0,
    //         rdev: 0,
    //         flags: 0,
    //     },
    // };

    // let some_file = File {
    //     name: String::from("some_file.txt"),
    //     attr: FileAttr {
    //         ino: 10,
    //         kind: FileType::RegularFile,
    //         size: 1000,
    //         blocks: 0,
    //         atime: Timespec::new(1, 0),
    //         mtime: Timespec::new(1, 0),
    //         ctime: Timespec::new(1, 0),
    //         crtime: Timespec::new(1, 0),
    //         perm: 0o644,
    //         nlink: 0,
    //         uid: 0,
    //         gid: 0,
    //         rdev: 0,
    //         flags: 0,
    //     },
    // };

    // let other_file = File {
    //     name: String::from("other_file.txt"),
    //     attr: FileAttr {
    //         ino: 11,
    //         kind: FileType::RegularFile,
    //         size: 1000,
    //         blocks: 0,
    //         atime: Timespec::new(1, 0),
    //         mtime: Timespec::new(1, 0),
    //         ctime: Timespec::new(1, 0),
    //         crtime: Timespec::new(1, 0),
    //         perm: 0o644,
    //         nlink: 0,
    //         uid: 0,
    //         gid: 0,
    //         rdev: 0,
    //         flags: 0,
    //     },
    // };

    // let mut tree: Tree<Inode> = TreeBuilder::new().with_node_capacity(5).build();

    // let wd_id: NodeId = tree.insert(Node::new(wd.inode()), AsRoot).unwrap();
    // let some_file_id = tree.insert(Node::new(some_file.inode()), UnderNode(&wd_id))
    //     .unwrap();
    // let other_file_id = tree.insert(Node::new(other_file.inode()), UnderNode(&wd_id))
    //     .unwrap();
    // let hello_dir_id = tree.insert(Node::new(hello_dir.inode()), UnderNode(&wd_id))
    //     .unwrap();
    // let world_file_id = tree.insert(Node::new(world_file.inode()), UnderNode(&hello_dir_id))
    //     .unwrap();

    // let ids = hashmap!{
    //     wd.inode() => wd_id,
    //     some_file.inode() => some_file_id,
    //     other_file.inode() => other_file_id,
    //     hello_dir.inode() => hello_dir_id,
    //     world_file.inode() => world_file_id,
    // };

    // let files = hashmap!{
    //     wd.inode() => wd,
    //     some_file.inode() => some_file,
    //     other_file.inode() => other_file,
    //     hello_dir.inode() => hello_dir,
    //     world_file.inode() => world_file,
    // };

    assert_eq!(2 + 2, 4);
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
