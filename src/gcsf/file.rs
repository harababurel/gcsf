use chrono::DateTime;
use drive3;
use failure::{err_msg, Error};
use fuse::{FileAttr, FileType};
use id_tree::NodeId;
use std::collections::HashMap;
use time::Timespec;

type Inode = u64;
type DriveId = String;

/// The representation of a local file used by GCSF.
///
/// `name`: the file name
/// `attr`: the file attributes,
/// `identical_name_id`: if there are multiple files with the same name, this attribute indicates
/// an additional numeric identifier for this particular file. This identifier influences the
/// reported file name (e.g some_file.txt.1)
/// `drive_file`: the associated Drive file (if one exists)
#[derive(Debug, Clone)]
pub struct File {
    pub name: String,
    pub attr: FileAttr,
    pub identical_name_id: Option<usize>,
    pub drive_file: Option<drive3::File>,
}

/// Specifies multiple ways of identifying a file:
///
/// * by inode
/// * by Drive ID
/// * by Node ID (the ID stored in the file tree)
/// * by parent inode + file name (as required by some fuse methods)
///
/// These types are somewhat equivalent and can be converted into one another.
#[derive(Debug, Clone)]
pub enum FileId {
    Inode(Inode),
    DriveId(String),
    NodeId(NodeId),
    ParentAndName { parent: Inode, name: String },
}

lazy_static! {
    static ref EXTENSIONS: HashMap<&'static str, &'static str> = hashmap! {
            "application/vnd.google-apps.document" => "#.odt",
            "application/vnd.google-apps.presentation" => "#.odp",
            "application/vnd.google-apps.spreadsheet" => "#.ods",
    };
}

impl File {
    /// Creates a new file using a Drive file as a template.
    pub fn from_drive_file(inode: Inode, drive_file: drive3::File) -> Self {
        let mut size = drive_file
            .size
            .clone()
            .map(|size| size.parse::<u64>().unwrap_or_default())
            .unwrap_or(10 * 1024 * 1024);

        let kind =
            if drive_file.mime_type == Some(String::from("application/vnd.google-apps.folder")) {
                size = 512;
                FileType::Directory
            } else {
                FileType::RegularFile
            };

        let times: Vec<_> = vec![
            &drive_file.created_time,
            &drive_file.modified_time,
            &drive_file.viewed_by_me_time,
        ]
        .iter()
        .map(
            |time| match DateTime::parse_from_rfc3339(time.as_ref().unwrap_or(&String::new())) {
                Ok(t) => Timespec {
                    sec: t.timestamp(),
                    nsec: t.timestamp_subsec_nanos() as i32,
                },
                Err(_) => Timespec { sec: 0, nsec: 0 },
            },
        )
        .collect();

        let (crtime, mtime, atime) = (times[0], times[1], times[2]);
        let bsize = 512;

        let mut attr = FileAttr {
            ino: inode,
            size,
            blocks: size / bsize + if size % bsize > 0 { 1 } else { 0 },
            atime,
            mtime,
            ctime: mtime,   // Time of last change
            crtime: crtime, // Time of creation (macOS only)
            kind,
            perm: 0o755,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
        };

        if attr.kind == FileType::Directory {
            attr.size = 512;
        }

        let mut filename = drive_file.name.clone().unwrap();
        // let owners: Vec<String> = drive_file
        //     .owners
        //     .clone()
        //     .unwrap()
        //     .into_iter()
        //     .map(|owner| owner.email_address.unwrap())
        //     .collect();

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
            identical_name_id: None,
            drive_file: Some(drive_file),
        }
    }

    /// Whether a character can be used in a valid POSIX file name.
    /// Read the [Wikipedia article](https://en.wikipedia.org/wiki/Filename)
    fn is_posix(c: &char) -> bool {
        let forbidden = String::from("*/:<>?\\|");
        !forbidden.contains(*c)
    }

    /// Whether this file is trashed on Drive.
    pub fn is_trashed(&self) -> bool {
        self.drive_file
            .as_ref()
            .map(|f| f.trashed)
            .unwrap_or_default()
            .unwrap_or(false)
    }

    // Trashing a file does not trigger a file update from Drive. Therefore this field must be
    // set manually so that GCSF knows that this particular file is trashed and should be deleted
    // permanently the next time unlink() is called.
    pub fn set_trashed(&mut self, trashed: bool) -> Result<(), Error> {
        let ino = self.inode();
        if let Some(ref mut drive_file) = self.drive_file.as_mut() {
            drive_file.trashed = Some(trashed);
            Ok(())
        } else {
            Err(err_msg(format!(
                "Could not set trashed={} because there is no drive file associated to {:?}",
                trashed,
                FileId::Inode(ino)
            )))
        }
    }

    #[allow(dead_code)]
    pub fn is_drive_document(&self) -> bool {
        self.drive_file
            .as_ref()
            .and_then(|f| f.mime_type.clone())
            .map(|t| EXTENSIONS.contains_key::<str>(&t))
            == Some(true)
    }

    pub fn name(&self) -> String {
        match self.identical_name_id {
            Some(id) => format!("{}.{}", self.name, id),
            None => self.name.clone(),
        }
    }

    pub fn inode(&self) -> Inode {
        self.attr.ino
    }

    pub fn kind(&self) -> FileType {
        self.attr.kind
    }

    pub fn drive_parent(&self) -> Option<String> {
        if self.drive_file.is_none() {
            return None;
        }

        self.drive_file
            .clone()
            .unwrap()
            .parents
            .and_then(|parents| parents.iter().take(1).next().cloned())
    }

    pub fn drive_id(&self) -> Option<String> {
        if self.drive_file.is_none() {
            return None;
        }

        self.drive_file.as_ref().unwrap().id.clone()
    }

    pub fn set_drive_id(&mut self, id: DriveId) {
        if self.drive_file.is_none() {
            return;
        }

        self.drive_file.as_mut().unwrap().id = Some(id);
    }

    #[allow(dead_code)]
    pub fn mime_type(&self) -> Option<String> {
        if self.drive_file.is_none() {
            return None;
        }

        self.drive_file.as_ref().unwrap().mime_type.clone()
    }
}
