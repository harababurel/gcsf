use drive3;
use fuse::{FileAttr, FileType};
use time::Timespec;

type Inode = u64;

#[derive(Clone)]
pub struct File {
    pub name: String,
    pub attr: FileAttr,
    pub drive_file: Option<drive3::File>,
}

impl File {
    pub fn from_drive_file(inode: Inode, drive_file: drive3::File) -> Self {
        let size = drive_file
            .size
            .clone()
            .map(|size| size.parse::<u64>().unwrap_or_default())
            .unwrap_or(0);

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

        File {
            name: drive_file.name.clone().unwrap(),
            attr,
            drive_file: Some(drive_file),
        }
    }

    pub fn inode(&self) -> Inode {
        self.attr.ino
    }

    pub fn kind(&self) -> FileType {
        self.attr.kind
    }
}
