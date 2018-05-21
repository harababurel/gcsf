use drive3;
use fuse::{FileAttr, FileType};
use id_tree::NodeId;
use std::collections::HashMap;
use time::Timespec;

type Inode = u64;

#[derive(Debug, Clone)]
pub struct File {
    pub name: String,
    pub attr: FileAttr,
    pub drive_file: Option<drive3::File>,
}

#[derive(Debug)]
pub enum FileId {
    Inode(Inode),
    DriveId(String),
    NodeId(NodeId),
    ParentAndName { parent: Inode, name: String },
}

lazy_static! {
    static ref EXTENSIONS: HashMap<&'static str, &'static str> = hashmap!{
            "application/vnd.google-apps.document" => ".docx",
            "application/vnd.google-apps.presentation" => ".pptx",
            "application/vnd.google-apps.spreadsheet" => ".xlsx",
    };
}

impl File {
    pub fn from_drive_file(inode: Inode, drive_file: drive3::File) -> Self {
        let size = drive_file
            .size
            .clone()
            .map(|size| size.parse::<u64>().unwrap_or_default())
            .unwrap_or(10 * 1024 * 1024);

        let attr = FileAttr {
            ino: inode,
            size,
            blocks: 1,
            atime: Timespec { sec: 0, nsec: 0 },
            mtime: Timespec { sec: 0, nsec: 0 },
            ctime: Timespec { sec: 0, nsec: 0 },
            crtime: Timespec { sec: 0, nsec: 0 },
            kind: if drive_file.mime_type == Some("application/vnd.google-apps.folder".to_string())
            {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: 0o755,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
        };

        let mut filename = drive_file.name.clone().unwrap();
        let owners: Vec<String> = drive_file
            .owners
            .clone()
            .unwrap()
            .into_iter()
            .map(|owner| owner.email_address.unwrap())
            .collect();

        let ext = drive_file
            .mime_type
            .clone()
            .and_then(|t| EXTENSIONS.get::<str>(&t));
        if ext.is_some() {
            filename = format!("{}{}", filename, ext.unwrap());
        }

        File {
            // name: format!("{} ({})", filename, owners.join(", ")),
            name: filename
                .chars()
                .filter(|c| File::is_posix(c))
                .collect::<String>(),
            attr,
            drive_file: Some(drive_file),
        }
    }

    pub fn is_posix(c: &char) -> bool {
        // https://en.wikipedia.org/wiki/Filename
        // @NTFS

        let forbidden = String::from("*/:<>?\\|");
        !forbidden.contains(*c)

        // (&'a' <= c && c <= &'z') || (&'A' <= c && c <= &'Z') || (&'0' <= c && c <= &'9')
        //     || c == &'.' || c == &'_' || c == &'-' || c == &' '
    }

    pub fn is_drive_document(&self) -> bool {
        self.drive_file
            .as_ref()
            .and_then(|f| f.mime_type.clone())
            .map(|t| EXTENSIONS.contains_key::<str>(&t)) == Some(true)
    }

    pub fn inode(&self) -> Inode {
        self.attr.ino
    }

    pub fn kind(&self) -> FileType {
        self.attr.kind
    }

    pub fn drive_id(&self) -> Option<String> {
        if self.drive_file.is_none() {
            return None;
        }

        self.drive_file.as_ref().unwrap().id.clone()
    }

    pub fn mime_type(&self) -> Option<String> {
        if self.drive_file.is_none() {
            return None;
        }

        self.drive_file.as_ref().unwrap().mime_type.clone()
    }
}
