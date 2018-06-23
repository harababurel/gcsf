use super::{File, FileId};
use drive3;
use failure::{err_msg, Error};
use fuse::{FileAttr, FileType};
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use id_tree::{Node, NodeId, Tree, TreeBuilder};
use std::collections::HashMap;
use std::collections::LinkedList;
use std::fmt;
use std::time::{Duration, SystemTime};
use time::Timespec;
use DriveFacade;

pub type Inode = u64;
pub type DriveId = String;

const ROOT_INODE: Inode = 1;
const TRASH_INODE: Inode = 2;

macro_rules! unwrap_or_continue {
    ($res:expr) => {
        match $res {
            Some(val) => val,
            None => {
                warn!("unwrap_or_continue!(): skipped.");
                continue;
            }
        }
    };
}

pub struct FileManager {
    tree: Tree<Inode>,
    pub files: HashMap<Inode, File>,
    pub node_ids: HashMap<Inode, NodeId>,
    pub drive_ids: HashMap<DriveId, Inode>,
    pub df: DriveFacade,
    pub last_sync: SystemTime,
    pub sync_interval: Duration,
}

/// Deals with everything that involves local file managing. In turn, uses a DriveFacade in order
/// to ensure consistency between the local and remote (drive) state.
impl FileManager {
    pub fn with_drive_facade(sync_interval: Duration, df: DriveFacade) -> Self {
        let mut manager = FileManager {
            tree: TreeBuilder::new().with_node_capacity(500).build(),
            files: HashMap::new(),
            node_ids: HashMap::new(),
            drive_ids: HashMap::new(),
            last_sync: SystemTime::now(),
            sync_interval,
            df,
        };

        if let Err(e) = manager.populate() {
            error!("Could not populate filesystem: {}", e);
        }

        if let Err(e) = manager.populate_trash() {
            error!("Could not populate trash dir: {}", e);
        }

        manager
    }

    pub fn sync(&mut self) -> Result<(), Error> {
        if SystemTime::now().duration_since(self.last_sync).unwrap() < self.sync_interval {
            return Err(err_msg(
                "Not enough time has passed since last sync. Will do nothing.",
            ));
        }

        info!("Checking for changes and possibly applying them.");
        self.last_sync = SystemTime::now();

        for change in self.df
            .get_all_changes()?
            .into_iter()
            .filter(|change| (&change).file.is_some())
        {
            debug!("Processing a change from {:?}", &change.time);
            let id = FileId::DriveId(change.file_id.unwrap());
            let drive_f = change.file.unwrap();

            // New file. Create it locally
            if !self.contains(&id) {
                debug!("New file. Create it locally");
                let f = File::from_drive_file(self.next_available_inode(), drive_f.clone());
                debug!("newly created file: {:#?}", &f);

                let parent = f.drive_parent().unwrap();
                debug!("drive parent: {:#?}", &parent);
                self.add_file(f, Some(FileId::DriveId(parent)))?;
                debug!("self.add_file() finished");
            }

            // Trashed file. Move it to trash locally
            if Some(true) == drive_f.trashed {
                debug!("Trashed file. Move it to trash locally");
                let result = self.move_file_to_trash(&id, false);
                if result.is_err() {
                    error!("Could not move to trash: {:?}", result)
                }
                continue;
            }

            // Removed file. Remove it locally.
            if let Some(true) = change.removed {
                debug!("Removed file. Remove it locally.");
                let result = self.delete_locally(&id);
                if result.is_err() {
                    error!("Could not delete locally: {:?}", result)
                }
                continue;
            }

            // Anything else: reconstruct the file locally and move it under its parent.
            debug!("Anything else: reconstruct the file locally and move it under its parent.");
            let new_parent = {
                let mut f = unwrap_or_continue!(self.get_mut_file(&id));
                *f = File::from_drive_file(f.inode(), drive_f.clone());
                FileId::DriveId(f.drive_parent().unwrap())
            };
            let result = self.move_locally(&id, &new_parent);
            if result.is_err() {
                error!("Could not move locally: {:?}", result)
            }
        }

        Ok(())
    }

    // Recursively adds all files and directories shown in "My Drive".
    fn populate(&mut self) -> Result<(), Error> {
        let root = self.new_root_file()?;
        self.add_file(root, None)?;

        let mut queue: LinkedList<DriveId> = LinkedList::new();
        queue.push_back(self.df.root_id().unwrap_or(&"root".to_string()).to_string());

        while !queue.is_empty() {
            let mut parents = Vec::new();
            while !queue.is_empty() {
                parents.push(queue.pop_front().unwrap());
            }

            for drive_file in self.df.get_all_files(Some(parents), Some(false))? {
                let mut file = File::from_drive_file(self.next_available_inode(), drive_file);

                if file.kind() == FileType::Directory {
                    queue.push_back(file.drive_id().unwrap());
                }

                // TODO: this makes everything slow; find a better solution
                // if file.is_drive_document() {
                //     let size = drive_facade
                //         .get_file_size(file.drive_id().as_ref().unwrap(), file.mime_type());
                //     file.attr.size = size;
                // }

                let file_parent = file.drive_parent().unwrap();
                if self.contains(&FileId::DriveId(file_parent.clone())) {
                    self.add_file(file, Some(FileId::DriveId(file_parent.clone())))?;
                } else {
                    self.add_file(file, None)?;
                }
            }
        }

        Ok(())
    }

    fn populate_trash(&mut self) -> Result<(), Error> {
        let root_id = self.df.root_id()?.to_string();
        let trash = self.new_special_dir("Trash", Some(TRASH_INODE));
        self.add_file(trash.clone(), Some(FileId::DriveId(root_id.to_string())))?;

        for drive_file in self.df.get_all_files(None, Some(true))? {
            let mut file = File::from_drive_file(self.next_available_inode(), drive_file);
            self.add_file(file, Some(FileId::Inode(trash.inode())))?;
        }

        Ok(())
    }

    fn new_root_file(&mut self) -> Result<File, Error> {
        let mut drive_file = drive3::File::default();

        let fallback_id = String::from("root");
        let root_id = self.df.root_id().unwrap_or(&fallback_id);
        drive_file.id = Some(root_id.to_string());

        Ok(File {
            name: String::from("."),
            attr: FileAttr {
                ino: ROOT_INODE,
                size: 4096,
                blocks: 1,
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
            identical_name_id: None,
            drive_file: Some(drive_file),
        })
    }

    fn new_special_dir(&mut self, name: &str, preferred_inode: Option<Inode>) -> File {
        File {
            name: name.to_string(),
            attr: FileAttr {
                ino: preferred_inode.unwrap_or(self.next_available_inode()),
                size: 4096,
                blocks: 1,
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
            identical_name_id: None,
            drive_file: None,
        }
    }

    pub fn next_available_inode(&self) -> Inode {
        (3..)
            .filter(|inode| !self.contains(&FileId::Inode(*inode)))
            .take(1)
            .next()
            .unwrap()
    }

    pub fn contains(&self, file_id: &FileId) -> bool {
        match file_id {
            FileId::Inode(inode) => self.node_ids.contains_key(&inode),
            FileId::DriveId(drive_id) => self.drive_ids.contains_key(drive_id),
            FileId::NodeId(node_id) => self.tree.get(&node_id).is_ok(),
            pn @ FileId::ParentAndName { .. } => self.get_file(&pn).is_some(),
        }
    }

    pub fn get_node_id(&self, file_id: &FileId) -> Option<NodeId> {
        match file_id {
            FileId::Inode(inode) => self.node_ids.get(&inode).cloned(),
            FileId::DriveId(drive_id) => self.get_node_id(&FileId::Inode(self.get_inode(
                &FileId::DriveId(drive_id.to_string()),
            ).unwrap())),
            FileId::NodeId(node_id) => Some(node_id.clone()),
            ref pn @ FileId::ParentAndName { .. } => {
                let inode = self.get_inode(&pn)?;
                self.get_node_id(&FileId::Inode(inode))
            }
        }
    }

    pub fn get_drive_id(&self, id: &FileId) -> Option<DriveId> {
        self.get_file(id)?.drive_id()
    }

    pub fn get_inode(&self, id: &FileId) -> Option<Inode> {
        match id {
            FileId::Inode(inode) => Some(*inode),
            FileId::DriveId(drive_id) => self.drive_ids.get(drive_id).cloned(),
            FileId::NodeId(node_id) => self.tree
                .get(&node_id)
                .map(|node| node.data())
                .ok()
                .cloned(),
            FileId::ParentAndName {
                ref parent,
                ref name,
            } => self.get_children(&FileId::Inode(*parent))?
                .into_iter()
                .find(|child| child.name() == *name)
                .map(|child| child.inode()),
        }
    }

    pub fn get_children(&self, id: &FileId) -> Option<Vec<&File>> {
        let node_id = self.get_node_id(&id)?;
        let children: Vec<&File> = self.tree
            .children(&node_id)
            .unwrap()
            .map(|child| self.get_file(&FileId::Inode(*child.data())))
            .filter(Option::is_some)
            .map(Option::unwrap)
            .collect();

        Some(children)
    }

    pub fn get_file(&self, id: &FileId) -> Option<&File> {
        let inode = self.get_inode(id)?;
        self.files.get(&inode)
    }

    pub fn get_mut_file(&mut self, id: &FileId) -> Option<&mut File> {
        let inode = self.get_inode(&id)?;
        self.files.get_mut(&inode)
    }

    /// Creates a file on Drive and adds it to the local file tree.
    pub fn create_file(&mut self, mut file: File, parent: Option<FileId>) -> Result<(), Error> {
        let drive_id = self.df.create(file.drive_file.as_ref().unwrap())?;
        file.set_drive_id(drive_id);
        self.add_file(file, parent)?;

        Ok(())
    }

    pub fn flush(&mut self, id: &FileId) -> Result<(), Error> {
        let file = self.get_drive_id(&id)
            .ok_or(err_msg(format!("Cannot find drive id of {:?}", &id)))?;
        self.df.flush(&file)
    }

    /// Adds a file to the local file tree. Does not communicate with Drive.
    fn add_file(&mut self, mut file: File, parent: Option<FileId>) -> Result<(), Error> {
        let node_id = match parent {
            Some(id) => {
                let parent_id = self.get_node_id(&id).ok_or(err_msg(
                    "FileManager::add_file() could not find parent by FileId",
                ))?;

                let identical_filename_count = self.get_children(&id)
                    .ok_or(err_msg(
                        "FileManager::add_file() could not get file siblings",
                    ))?
                    .iter()
                    .filter(|child| child.name == file.name)
                    .count();

                if identical_filename_count > 0 {
                    file.identical_name_id = Some(identical_filename_count);
                }

                self.tree
                    .insert(Node::new(file.inode()), UnderNode(&parent_id))?
            }
            None => self.tree.insert(Node::new(file.inode()), AsRoot)?,
        };

        self.node_ids.insert(file.inode(), node_id);
        file.drive_id()
            .and_then(|drive_id| self.drive_ids.insert(drive_id, file.inode()));
        self.files.insert(file.inode(), file);

        Ok(())
    }

    pub fn move_locally(&mut self, id: &FileId, new_parent: &FileId) -> Result<(), Error> {
        let current_node = self.get_node_id(&id)
            .ok_or(err_msg(format!("Cannot find node_id of {:?}", &id)))?;
        let target_node = self.get_node_id(&new_parent)
            .ok_or(err_msg("Target node doesn't exist"))?;

        self.tree.move_node(&current_node, ToParent(&target_node))?;
        Ok(())
    }

    pub fn delete_locally(&mut self, id: &FileId) -> Result<(), Error> {
        let node_id = self.get_node_id(id)
            .ok_or(err_msg(format!("Cannot find node_id of {:?}", &id)))?;
        let inode = self.get_inode(id)
            .ok_or(err_msg(format!("Cannot find inode of {:?}", &id)))?;
        let drive_id = self.get_drive_id(id)
            .ok_or(err_msg(format!("Cannot find drive id of {:?}", &id)))?;

        self.tree.remove_node(node_id, DropChildren)?;
        self.files.remove(&inode);
        self.node_ids.remove(&inode);
        self.drive_ids.remove(&drive_id);

        Ok(())
    }

    pub fn delete(&mut self, id: &FileId) -> Result<(), Error> {
        let drive_id = self.get_drive_id(id).ok_or(err_msg("No such file"))?;

        self.delete_locally(id)?;
        match self.df.delete_permanently(&drive_id) {
            Ok(response) => {
                debug!("{:?}", response);
                Ok(())
            }
            Err(e) => Err(err_msg(format!("{}", e))),
        }
    }

    pub fn move_file_to_trash(&mut self, id: &FileId, also_on_drive: bool) -> Result<(), Error> {
        debug!("Moving {:?} to trash.", &id);
        let node_id = self.get_node_id(id)
            .ok_or(err_msg(format!("Cannot find node_id of {:?}", &id)))?;
        let drive_id = self.get_drive_id(id)
            .ok_or(err_msg(format!("Cannot find drive_id of {:?}", &id)))?;
        let trash_id = self.get_node_id(&FileId::Inode(TRASH_INODE))
            .ok_or(err_msg("Cannot find node_id of Trash dir"))?;

        self.tree.move_node(&node_id, ToParent(&trash_id))?;

        if also_on_drive {
            self.df.move_to_trash(drive_id)?;
        }

        Ok(())
    }

    pub fn rename(
        &mut self,
        id: &FileId,
        new_parent: Inode,
        new_name: String,
    ) -> Result<(), Error> {
        // Identify the file by its inode instead of (parent, name) because both the parent and
        // name will probably change in this method.
        let id = FileId::Inode(self.get_inode(id)
            .ok_or(err_msg(format!("Cannot find node_id of {:?}", &id)))?);

        let current_node = self.get_node_id(&id)
            .ok_or(err_msg(format!("Cannot find node_id of {:?}", &id)))?;
        let target_node = self.get_node_id(&FileId::Inode(new_parent))
            .ok_or(err_msg("Target node doesn't exist"))?;

        self.tree.move_node(&current_node, ToParent(&target_node))?;

        {
            let identical_filename_count = self.get_children(&FileId::Inode(new_parent))
                .ok_or(err_msg("FileManager::rename() could not get file siblings"))?
                .iter()
                .filter(|child| child.name == new_name)
                .count();

            let file = self.get_mut_file(&id).ok_or(err_msg("File doesn't exist"))?;
            file.name = new_name.clone();

            if identical_filename_count > 0 {
                file.identical_name_id = Some(identical_filename_count);
            } else {
                file.identical_name_id = None;
            }
        }

        let drive_id = self.get_drive_id(&id)
            .ok_or(err_msg(format!("Cannot find drive_id of {:?}", &id)))?;
        let parent_id = self.get_drive_id(&FileId::Inode(new_parent))
            .ok_or(err_msg(format!(
                "Cannot find drive_id of {:?}",
                &FileId::Inode(new_parent)
            )))?;

        debug!("parent_id: {}", &parent_id);
        self.df.move_to(&drive_id, &parent_id, &new_name)?;
        Ok(())
    }

    pub fn write(&mut self, id: FileId, offset: usize, data: &[u8]) {
        let drive_id = self.get_drive_id(&id).unwrap();
        self.df.write(drive_id, offset, data);
    }
}

impl fmt::Debug for FileManager {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FileManager(\n")?;

        if self.tree.root_node_id().is_none() {
            return write!(f, ")\n");
        }

        let mut stack: Vec<(u32, &NodeId)> = vec![(0, self.tree.root_node_id().unwrap())];

        while !stack.is_empty() {
            let (level, node_id) = stack.pop().unwrap();

            for _ in 0..level {
                write!(f, "\t")?;
            }

            let file = self.get_file(&FileId::NodeId(node_id.clone())).unwrap();
            write!(f, "{:3} => {}\n", file.inode(), file.name)?;

            self.tree.children_ids(node_id).unwrap().for_each(|id| {
                stack.push((level + 1, id));
            });
        }

        write!(f, ")\n")
    }
}
