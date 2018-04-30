use fuse::{FileAttr, FileType};
type Inode = u64;

#[derive(Clone)]
pub struct File {
    pub name: String,
    pub attr: FileAttr,
    pub pieces: Vec<String>, // filename of each piece of this file on google drive
}

impl File {
    pub fn inode(&self) -> Inode {
        self.attr.ino
    }

    pub fn kind(&self) -> FileType {
        self.attr.kind
    }
}
