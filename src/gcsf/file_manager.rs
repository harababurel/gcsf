use super::{Checksums, File, FileId};
use crate::DriveFacade;
use crate::drive3;
use failure::{Error, err_msg};
use fuser::{FileAttr, FileType, INodeNo};
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use id_tree::{Node, NodeId, Tree, TreeBuilder};
use std::collections::HashMap;
use std::collections::LinkedList;
use std::fmt;
use std::time::{Duration, SystemTime};

pub type Inode = u64;
pub type DriveId = String;

const ROOT_INODE: Inode = 1;
const TRASH_INODE: Inode = 2;
const SHARED_INODE: Inode = 3;

/// Manages files locally and uses a DriveFacade in order to communicate with Google Drive and to ensure consistency between the local and remote state.
pub struct FileManager {
    /// A representation of the file tree. Each tree node stores the inode of the corresponding file.
    tree: Tree<Inode>,

    /// Maps inodes to the corresponding files.
    pub files: HashMap<Inode, File>,

    /// Maps inodes to corresponding node ids that `tree` uses.
    pub node_ids: HashMap<Inode, NodeId>,

    /// Maps Google Drive ids (i.e strings) to corresponding inodes.
    pub drive_ids: HashMap<DriveId, Inode>,

    /// A `DriveFacade` is used in order to communicate with the Google Drive API.
    pub df: DriveFacade,

    /// The last timestamp when the file manager asked Google Drive for remote changes.
    pub last_sync: SystemTime,

    /// Specifies how much time is needed to pass since `last_sync` for a new sync to be performed.
    pub sync_interval: Duration,

    /// Rename duplicate files if enabled.
    pub rename_identical_files: bool,

    /// Add an extension to special files (docs, presentations, sheets, drawings, sites).
    /// e.g. "#.ods" for spreadsheets.
    pub add_extensions_to_special_files: bool,

    /// If enabled, deleting files will remove them permanently instead of moving them to Trash.
    /// Deleting trashed files always removes them permanently.
    pub skip_trash: bool,

    last_inode: Inode,
}

impl FileManager {
    /// Creates a new FileManager with a specific `sync_interval` and an injected `DriveFacade`.
    /// Also populates the manager's file tree with files contained in "My Drive" and "Trash".
    pub fn with_drive_facade(
        rename_identical_files: bool,
        add_extensions_to_special_files: bool,
        skip_trash: bool,
        sync_interval: Duration,
        df: DriveFacade,
    ) -> Result<Self, Error> {
        let mut manager = FileManager {
            tree: TreeBuilder::new().with_node_capacity(500).build(),
            files: HashMap::new(),
            node_ids: HashMap::new(),
            drive_ids: HashMap::new(),
            last_sync: SystemTime::now(),
            rename_identical_files,
            add_extensions_to_special_files,
            skip_trash,
            sync_interval,
            df,
            last_inode: SHARED_INODE, // Must be >= max reserved inode (ROOT=1, TRASH=2, SHARED=3)
        };

        manager
            .populate()
            .map_err(|e| err_msg(format!("Could not populate file system:\n{}", e)))?;
        manager
            .populate_trash()
            .map_err(|e| err_msg(format!("Could not populate trash dir:\n{}", e)))?;
        Ok(manager)
    }

    /// Tries to retrieve recent changes from the `DriveFacade` and apply them locally in order to
    /// maintain data consistency. Fails early if not enough time has passed since the last sync.
    pub fn sync(&mut self) -> Result<(), Error> {
        let now = SystemTime::now();
        if now.duration_since(self.last_sync).unwrap_or_default() < self.sync_interval {
            return Err(err_msg(
                "Not enough time has passed since last sync. Will do nothing.",
            ));
        }

        info!("Checking for changes and possibly applying them.");
        let (changes, next_changes_token) = self.df.get_all_changes()?;

        for change in changes {
            debug!("Processing a change from {:?}", change.time);
            let drive_id = change
                .file_id
                .ok_or_else(|| err_msg("Drive change did not include a file id"))?;
            let id = FileId::DriveId(drive_id);

            if change.removed == Some(true) {
                debug!("Removed file. Remove it locally.");
                if self.contains(&id) {
                    self.delete_locally(&id)?;
                }
                continue;
            }

            let drive_f = change
                .file
                .ok_or_else(|| err_msg("Drive change did not include file metadata"))?;
            self.adopt_checksums_from_change(&id, &drive_f);

            // New file. Create it locally
            if !self.contains(&id) {
                debug!("New file. Create it locally");
                let f = File::from_drive_file(
                    self.next_available_inode(),
                    drive_f.clone(),
                    self.add_extensions_to_special_files,
                );
                debug!("newly created file: {:#?}", f);

                let parent = f
                    .drive_parent()
                    .ok_or_else(|| err_msg("Changed Drive file did not include a parent"))?;
                debug!("drive parent: {:#?}", parent);
                self.add_file_locally(f, Some(FileId::DriveId(parent.clone())))?;
                debug!("self.add_file_locally() finished");

                // Recalculate suffixes for the parent directory
                if self.rename_identical_files
                    && let Some(parent_inode) = self.get_inode(&FileId::DriveId(parent))
                {
                    self.recalculate_duplicate_suffixes_for_parent(parent_inode);
                }
            }

            // Trashed file. Move it to trash locally
            if Some(true) == drive_f.trashed {
                debug!("Trashed file. Move it to trash locally");
                self.move_file_to_trash(&id, false)?;
                continue;
            }

            // Anything else: reconstruct the file locally and move it under its parent.
            debug!("Anything else: reconstruct the file locally and move it under its parent.");
            let old_parent = self.get_parent_inode(&id);
            let new_parent = {
                let add_extension = self.add_extensions_to_special_files;
                let f = self
                    .get_mut_file(&id)
                    .ok_or_else(|| err_msg("Changed Drive file is missing from the local index"))?;
                *f = File::from_drive_file(f.inode(), drive_f.clone(), add_extension);
                FileId::DriveId(
                    f.drive_parent()
                        .ok_or_else(|| err_msg("Changed Drive file did not include a parent"))?,
                )
            };
            self.move_locally(&id, &new_parent)?;

            // Recalculate suffixes for both old and new parent directories
            if self.rename_identical_files {
                if let Some(old_parent_inode) = old_parent {
                    self.recalculate_duplicate_suffixes_for_parent(old_parent_inode);
                }
                if let Some(new_parent_inode) = self.get_inode(&new_parent) {
                    self.recalculate_duplicate_suffixes_for_parent(new_parent_inode);
                }
            }
        }

        self.df.commit_changes_token(next_changes_token);
        self.last_sync = now;
        Ok(())
    }

    /// Retrieves all files and directories shown in "My Drive" and "Shared with me" and adds them locally.
    fn populate(&mut self) -> Result<(), Error> {
        let root = self.new_root_file();
        self.add_file_locally(root, None)?;

        let shared = self.new_special_dir("Shared with me", Some(SHARED_INODE));
        self.add_file_locally(shared, Some(FileId::Inode(ROOT_INODE)))?;

        // Disable rename_identical_files during initial population to avoid
        // global duplicate detection. We'll recalculate per-directory after moves.
        let should_rename = self.rename_identical_files;
        self.rename_identical_files = false;

        for drive_file in self.df.get_all_files(None, Some(false))? {
            let file = File::from_drive_file(
                self.next_available_inode(),
                drive_file,
                self.add_extensions_to_special_files,
            );
            self.add_file_locally(file, Some(FileId::Inode(SHARED_INODE)))?;
        }

        let mut moves: LinkedList<(FileId, FileId)> = LinkedList::new();
        for (inode, file) in &self.files {
            if let Some(parent) = file.drive_parent()
                && self.contains(&FileId::DriveId(parent.clone()))
            {
                moves.push_back((FileId::Inode(*inode), FileId::DriveId(parent)));
            }
        }

        for (inode, parent) in &moves {
            if let Err(e) = self.move_locally(inode, parent) {
                error!("{}", e);
            }
        }

        // Restore setting and recalculate suffixes per-directory now that
        // files are in their final positions
        self.rename_identical_files = should_rename;
        if self.rename_identical_files {
            self.recalculate_all_duplicate_suffixes();
        }

        Ok(())
    }

    /// Retrieves all trashed files and directories and adds them locally in a special directory.
    fn populate_trash(&mut self) -> Result<(), Error> {
        let root_id = self.df.root_id()?.to_string();
        let trash = self.new_special_dir("Trash", Some(TRASH_INODE));
        self.add_file_locally(trash.clone(), Some(FileId::DriveId(root_id)))?;

        // Disable rename_identical_files during population, recalculate after
        let should_rename = self.rename_identical_files;
        self.rename_identical_files = false;

        for drive_file in self.df.get_all_files(None, Some(true))? {
            let file = File::from_drive_file(
                self.next_available_inode(),
                drive_file,
                self.add_extensions_to_special_files,
            );
            self.add_file_locally(file, Some(FileId::Inode(trash.inode())))?;
        }

        // Restore setting and recalculate suffixes for trash directory
        self.rename_identical_files = should_rename;
        if self.rename_identical_files {
            self.recalculate_duplicate_suffixes_for_parent(TRASH_INODE);
        }

        Ok(())
    }

    /// Creates a new File struct which represents the root directory. If possible, it fills in the exact DriveId. If not, it
    /// keeps using "root" as a placeholder id.
    fn new_root_file(&mut self) -> File {
        let mut drive_file = drive3::api::File::default();

        let fallback_id = String::from("root");
        let root_id = self.df.root_id().unwrap_or(&fallback_id);
        drive_file.id = Some(root_id.to_string());

        File {
            name: String::from("."),
            attr: FileAttr {
                ino: INodeNo(ROOT_INODE),
                size: 512,
                blocks: 1,
                blksize: 512,
                atime: SystemTime::UNIX_EPOCH,
                mtime: SystemTime::UNIX_EPOCH,
                ctime: SystemTime::UNIX_EPOCH,
                crtime: SystemTime::UNIX_EPOCH,
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
        }
    }

    /// Creates a new File struct which represents a directory that does not necessarily exist on
    /// Drive.
    fn new_special_dir(&mut self, name: &str, preferred_inode: Option<Inode>) -> File {
        File {
            name: name.to_string(),
            attr: FileAttr {
                ino: INodeNo(preferred_inode.unwrap_or_else(|| self.next_available_inode())),
                size: 512,
                blocks: 1,
                blksize: 512,
                atime: SystemTime::UNIX_EPOCH,
                mtime: SystemTime::UNIX_EPOCH,
                ctime: SystemTime::UNIX_EPOCH,
                crtime: SystemTime::UNIX_EPOCH,
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

    /// Returns the next unused inode.
    pub fn next_available_inode(&mut self) -> Inode {
        self.last_inode += 1;
        self.last_inode
    }

    /// Returns true if the file identified by a given id exists in the filesystem.
    pub fn contains(&self, file_id: &FileId) -> bool {
        match file_id {
            FileId::Inode(inode) => self.node_ids.contains_key(inode),
            FileId::DriveId(drive_id) => self.drive_ids.contains_key(drive_id),
            FileId::NodeId(node_id) => self.tree.get(node_id).is_ok(),
            pn @ FileId::ParentAndName { .. } => self.get_file(pn).is_some(),
        }
    }

    /// Returns the NodeId of a file identified by a given id.
    /// The NodeId indicates the placement of the file in the file tree.
    pub fn get_node_id(&self, file_id: &FileId) -> Option<NodeId> {
        match file_id {
            FileId::Inode(inode) => self.node_ids.get(inode).cloned(),
            FileId::DriveId(drive_id) => self.get_node_id(&FileId::Inode(
                self.get_inode(&FileId::DriveId(drive_id.to_string()))?,
            )),
            FileId::NodeId(node_id) => Some(node_id.clone()),
            pn => {
                let inode = self.get_inode(pn)?;
                self.get_node_id(&FileId::Inode(inode))
            }
        }
    }

    /// Returns the DriveId of a file identified by a given id.
    /// The DriveId points to a Google Drive file.
    pub fn get_drive_id(&self, id: &FileId) -> Option<DriveId> {
        self.get_file(id)?.drive_id()
    }

    /// Returns the inode of a file identified by a given id.
    pub fn get_inode(&self, id: &FileId) -> Option<Inode> {
        match id {
            FileId::Inode(inode) => Some(*inode),
            FileId::DriveId(drive_id) => self.drive_ids.get(drive_id).cloned(),
            FileId::NodeId(node_id) => self.tree.get(node_id).map(|node| node.data()).ok().cloned(),
            FileId::ParentAndName { parent, name } => self
                .get_children(&FileId::Inode(*parent))?
                .into_iter()
                .find(|child| child.name() == *name)
                .map(|child| child.inode()),
        }
    }

    /// Returns the children of a directory identified by a given id.
    pub fn get_children(&self, id: &FileId) -> Option<Vec<&File>> {
        let node_id = self.get_node_id(id)?;
        let children: Vec<&File> = self
            .tree
            .children(&node_id)
            .unwrap()
            .filter_map(|child| self.get_file(&FileId::Inode(*child.data())))
            .collect();

        Some(children)
    }

    /// Returns a const reference to a file identified by a given id.
    pub fn get_file(&self, id: &FileId) -> Option<&File> {
        let inode = self.get_inode(id)?;
        self.files.get(&inode)
    }

    /// Returns a mutable reference to a file identified by a given id.
    pub fn get_mut_file(&mut self, id: &FileId) -> Option<&mut File> {
        let inode = self.get_inode(id)?;
        self.files.get_mut(&inode)
    }

    /// Creates a file on Drive and adds it to the local file tree.
    pub fn create_file(&mut self, mut file: File, parent: Option<FileId>) -> Result<(), Error> {
        let drive_file = file
            .drive_file
            .as_ref()
            .ok_or_else(|| err_msg("Cannot create a file without Drive metadata"))?;
        let drive_id = self.df.create(drive_file)?;
        file.set_drive_id(drive_id.clone());
        if let Err(local_error) = self.add_file_locally(file, parent) {
            return match self.df.delete_permanently(&drive_id) {
                Ok(true) => Err(local_error),
                Ok(false) => Err(err_msg(format!(
                    "Could not add new file locally and Drive did not confirm cleanup: {}",
                    local_error
                ))),
                Err(cleanup_error) => Err(err_msg(format!(
                    "Could not add new file locally ({}) or clean it up on Drive ({})",
                    local_error, cleanup_error
                ))),
            };
        }

        Ok(())
    }

    /// Returns the Drive-computed checksums of a file's content, or `None` if the file is unknown,
    /// has no Drive counterpart, or has local modifications that Drive has not checksummed yet.
    pub fn checksums(&self, id: &FileId) -> Option<Checksums> {
        let drive_file = self.get_file(id)?.drive_file.as_ref()?;
        let drive_id = drive_file.id.as_ref()?;
        self.df.checksums(drive_id, Checksums::of(drive_file))
    }

    /// Change entries carry a file's current metadata, which is at least as recent as anything this
    /// instance uploaded before the changes were listed. The listed checksums therefore replace
    /// the ones recorded from uploads. Both are updated together so that a sync aborted midway
    /// cannot leave older listed checksums exposed.
    fn adopt_checksums_from_change(&mut self, id: &FileId, drive_f: &drive3::api::File) {
        if let Some(drive_file) = self
            .get_mut_file(id)
            .and_then(|file| file.drive_file.as_mut())
        {
            drive_file.md5_checksum = drive_f.md5_checksum.clone();
            drive_file.sha256_checksum = drive_f.sha256_checksum.clone();
        }
        if let FileId::DriveId(drive_id) = id {
            self.df.forget_uploaded_checksums(drive_id);
        }
    }

    /// Passes along the FLUSH system call to the `DriveFacade`.
    pub fn flush(&mut self, id: &FileId) -> Result<(), Error> {
        let file = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find drive id of {:?}", id)))?;
        self.df.flush(&file)
    }

    /// Flushes all staged file content to Drive.
    pub fn flush_all(&mut self) -> Result<(), Error> {
        self.df.flush_all()
    }

    /// Adds a file to the local file tree. Does not communicate with Drive.
    fn add_file_locally(&mut self, mut file: File, parent: Option<FileId>) -> Result<(), Error> {
        let node_id = match parent {
            Some(id) => {
                let parent_id = self.get_node_id(&id).ok_or_else(|| {
                    err_msg("FileManager::add_file_locally() could not find parent by FileId")
                })?;

                if self.rename_identical_files {
                    let identical_filename_count = self
                        .get_children(&id)
                        .ok_or_else(|| {
                            err_msg("FileManager::add_file_locally() could not get file siblings")
                        })?
                        .iter()
                        .filter(|child| child.name == file.name)
                        .count();

                    if identical_filename_count > 0 {
                        file.identical_name_id = Some(identical_filename_count);
                    }
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

    /// Moves a file somewhere else in the local file tree. Does not communicate with Drive.
    fn move_locally(&mut self, id: &FileId, new_parent: &FileId) -> Result<(), Error> {
        let current_node = self
            .get_node_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find node_id of {:?}", id)))?;
        let target_node = self
            .get_node_id(new_parent)
            .ok_or_else(|| err_msg("Target node doesn't exist"))?;

        self.tree.move_node(&current_node, ToParent(&target_node))?;
        Ok(())
    }

    /// Returns the parent inode of a file, if it has one.
    fn get_parent_inode(&self, id: &FileId) -> Option<Inode> {
        let node_id = self.get_node_id(id)?;
        let parent_node_id = self.tree.get(&node_id).ok()?.parent()?;
        self.tree.get(parent_node_id).ok().map(|n| *n.data())
    }

    /// Recalculates identical_name_id for all files in all directories.
    /// Uses Drive ID for stable, deterministic ordering.
    pub(crate) fn recalculate_all_duplicate_suffixes(&mut self) {
        // Collect all directory inodes first to avoid borrow issues
        let dir_inodes: Vec<Inode> = self
            .files
            .iter()
            .filter(|(_, f)| f.kind() == FileType::Directory)
            .map(|(ino, _)| *ino)
            .collect();

        for dir_inode in dir_inodes {
            self.recalculate_duplicate_suffixes_for_parent(dir_inode);
        }
    }

    /// Recalculates identical_name_id for all children of a given parent directory.
    /// Files are sorted by Drive ID for stable ordering (first by Drive ID = no suffix).
    pub(crate) fn recalculate_duplicate_suffixes_for_parent(&mut self, parent_inode: Inode) {
        // Get children and group by base name
        let children: Vec<(Inode, String, Option<String>)> = self
            .get_children(&FileId::Inode(parent_inode))
            .unwrap_or_default()
            .iter()
            .map(|f| (f.inode(), f.name.clone(), f.drive_id()))
            .collect();

        // Group by base name
        let mut by_name: HashMap<String, Vec<(Inode, Option<String>)>> = HashMap::new();
        for (inode, name, drive_id) in children {
            by_name.entry(name).or_default().push((inode, drive_id));
        }

        // Assign suffixes to duplicates
        for (name, mut entries) in by_name {
            if entries.len() <= 1 {
                // No duplicates - clear any existing suffix
                if let Some((inode, _)) = entries.first()
                    && let Some(f) = self.files.get_mut(inode)
                {
                    f.identical_name_id = None;
                }
                continue;
            }

            // Log detected duplicates
            info!(
                "Found {} files with identical name '{}' in directory (inode {})",
                entries.len(),
                name,
                parent_inode
            );

            // Sort by Drive ID for stable ordering (None sorts first)
            entries.sort_by(|a, b| a.1.cmp(&b.1));

            // First file gets no suffix, rest get .1, .2, etc.
            for (idx, (inode, _)) in entries.iter().enumerate() {
                if let Some(f) = self.files.get_mut(inode) {
                    f.identical_name_id = if idx == 0 { None } else { Some(idx) };
                }
            }
        }
    }

    /// Deletes a file and its children from the local file tree. Does not communicate with Drive.
    fn delete_locally(&mut self, id: &FileId) -> Result<(), Error> {
        let node_id = self
            .get_node_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find node_id of {:?}", id)))?;
        let old_parent = self.get_parent_inode(id);
        let inodes: Vec<Inode> = self
            .tree
            .traverse_pre_order(&node_id)?
            .map(|node| *node.data())
            .collect();
        let drive_ids: Vec<DriveId> = inodes
            .iter()
            .filter_map(|inode| self.files.get(inode).and_then(File::drive_id))
            .collect();

        self.tree.remove_node(node_id, DropChildren)?;
        for inode in inodes {
            self.files.remove(&inode);
            self.node_ids.remove(&inode);
        }
        for drive_id in drive_ids {
            self.drive_ids.remove(&drive_id);
            self.df.discard_file_state(&drive_id);
        }
        if self.rename_identical_files
            && let Some(old_parent) = old_parent
        {
            self.recalculate_duplicate_suffixes_for_parent(old_parent);
        }

        Ok(())
    }

    /// Deletes a file locally *and* on Drive.
    pub fn delete(&mut self, id: &FileId) -> Result<(), Error> {
        let drive_id = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg("No such file"))?;

        match self.df.delete_permanently(&drive_id) {
            Ok(true) => {
                self.delete_locally(id)?;
                Ok(())
            }
            Ok(false) => Err(err_msg("Drive did not confirm permanent deletion")),
            Err(e) => Err(err_msg(format!("{}", e))),
        }
    }

    /// Moves a file to the Trash directory locally *and* on Drive.
    pub fn move_file_to_trash(&mut self, id: &FileId, also_on_drive: bool) -> Result<(), Error> {
        debug!("Moving {:?} to trash.", id);
        let node_id = self
            .get_node_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find node_id of {:?}", id)))?;
        let drive_id = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find drive_id of {:?}", id)))?;
        let trash_id = self
            .get_node_id(&FileId::Inode(TRASH_INODE))
            .ok_or_else(|| err_msg("Cannot find node_id of Trash dir"))?;

        // Get the old parent inode before moving (for suffix recalculation)
        let old_parent_inode = self
            .tree
            .get(&node_id)?
            .parent()
            .and_then(|parent_node_id| self.tree.get(parent_node_id).ok())
            .map(|parent_node| *parent_node.data());

        if also_on_drive {
            self.df.flush(&drive_id)?;
            self.df.move_to_trash(drive_id.clone())?;
        }

        self.tree.move_node(&node_id, ToParent(&trash_id))?;

        // File cannot be identified by parent and name after moving it.
        self.get_mut_file(&FileId::DriveId(drive_id.clone()))
            .ok_or_else(|| err_msg(format!("Cannot find {:?}", drive_id)))?
            .set_trashed(true)?;

        // Recalculate suffixes for both old parent and Trash directories
        if self.rename_identical_files {
            if let Some(old_parent) = old_parent_inode {
                self.recalculate_duplicate_suffixes_for_parent(old_parent);
            }
            self.recalculate_duplicate_suffixes_for_parent(TRASH_INODE);
        }

        Ok(())
    }

    /// Whether a file is trashed on Drive.
    pub fn file_is_trashed(&mut self, id: &FileId) -> Result<bool, Error> {
        let file = self
            .get_file(id)
            .ok_or_else(|| err_msg(format!("Cannot find node_id of {:?}", id)))?;

        Ok(file.is_trashed())
    }

    /// Moves/renames a file locally *and* on Drive.
    pub fn rename(
        &mut self,
        id: &FileId,
        new_parent: Inode,
        new_name: String,
    ) -> Result<(), Error> {
        // Identify the file by its inode instead of (parent, name) because both the parent and
        // name will probably change in this method.
        let id = FileId::Inode(
            self.get_inode(id)
                .ok_or_else(|| err_msg(format!("Cannot find inode of {:?}", id)))?,
        );

        // Get old parent before moving (for suffix recalculation)
        let old_parent = self.get_parent_inode(&id);

        let current_node = self
            .get_node_id(&id)
            .ok_or_else(|| err_msg(format!("Cannot find node_id of {:?}", id)))?;
        let target_node = self
            .get_node_id(&FileId::Inode(new_parent))
            .ok_or_else(|| err_msg("Target node doesn't exist"))?;

        let drive_id = self
            .get_drive_id(&id)
            .ok_or_else(|| err_msg(format!("Cannot find drive_id of {:?}", id)))?;
        let parent_id = self
            .get_drive_id(&FileId::Inode(new_parent))
            .ok_or_else(|| {
                err_msg(format!(
                    "Cannot find drive_id of {:?}",
                    FileId::Inode(new_parent)
                ))
            })?;

        debug!("parent_id: {}", parent_id);
        self.df.move_to(&drive_id, &parent_id, &new_name)?;

        self.tree.move_node(&current_node, ToParent(&target_node))?;

        let file = self
            .get_mut_file(&id)
            .ok_or_else(|| err_msg("File doesn't exist"))?;
        file.name = new_name;
        file.identical_name_id = None;

        if self.rename_identical_files {
            if let Some(old_parent_inode) = old_parent {
                self.recalculate_duplicate_suffixes_for_parent(old_parent_inode);
            }
            self.recalculate_duplicate_suffixes_for_parent(new_parent);
        }

        Ok(())
    }

    /// Writes to a file locally *and* on Drive. Note: the pending write is not necessarily applied
    /// instantly by the `DriveFacade`.
    pub fn write(&mut self, id: &FileId, offset: usize, data: &[u8]) -> Result<(), Error> {
        let drive_id = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find drive id of {:?}", id)))?;
        self.df.write(drive_id, offset, data)
    }

    /// Writes data to a file and persists it before returning.
    pub fn write_and_flush(
        &mut self,
        id: &FileId,
        offset: usize,
        data: &[u8],
    ) -> Result<(), Error> {
        let drive_id = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find drive id of {:?}", id)))?;
        self.df.write_and_flush(drive_id, offset, data)
    }

    /// Changes a file's length and persists it before returning.
    pub fn truncate(&mut self, id: &FileId, size: usize) -> Result<(), Error> {
        let drive_id = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find drive id of {:?}", id)))?;
        self.df.truncate_and_flush(drive_id, size)
    }

    /// Renames a file and moves it into the virtual Trash directory.
    pub fn rename_and_move_to_trash(&mut self, id: &FileId, new_name: String) -> Result<(), Error> {
        let node_id = self
            .get_node_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find node_id of {:?}", id)))?;
        let trash_id = self
            .get_node_id(&FileId::Inode(TRASH_INODE))
            .ok_or_else(|| err_msg("Cannot find node_id of Trash dir"))?;
        let old_parent = self.get_parent_inode(id);
        let drive_id = self
            .get_drive_id(id)
            .ok_or_else(|| err_msg(format!("Cannot find drive_id of {:?}", id)))?;

        self.df.flush(&drive_id)?;
        self.df.move_to_trash_with_name(&drive_id, &new_name)?;
        self.tree.move_node(&node_id, ToParent(&trash_id))?;

        let file = self
            .get_mut_file(&FileId::DriveId(drive_id))
            .ok_or_else(|| err_msg("File disappeared after moving it to Trash"))?;
        file.name = new_name;
        file.identical_name_id = None;
        file.set_trashed(true)?;

        if self.rename_identical_files {
            if let Some(old_parent) = old_parent {
                self.recalculate_duplicate_suffixes_for_parent(old_parent);
            }
            self.recalculate_duplicate_suffixes_for_parent(TRASH_INODE);
        }

        Ok(())
    }

    #[cfg(test)]
    /// Create a FileManager with manual state for testing (no Drive API calls).
    /// This bypasses the normal constructor which makes Drive API calls during initialization.
    ///
    pub fn new_for_testing(rename_identical_files: bool) -> Self {
        let df = DriveFacade::new_for_testing();

        let mut tree = TreeBuilder::new().with_node_capacity(500).build();
        let mut files = HashMap::new();
        let mut node_ids = HashMap::new();

        // Create a root node so tests can add files
        let root_file = File {
            name: "/".to_string(),
            attr: FileAttr {
                ino: INodeNo(ROOT_INODE),
                size: 512,
                blocks: 1,
                blksize: 512,
                atime: SystemTime::UNIX_EPOCH,
                mtime: SystemTime::UNIX_EPOCH,
                ctime: SystemTime::UNIX_EPOCH,
                crtime: SystemTime::UNIX_EPOCH,
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
        };

        let root_node_id = tree.insert(Node::new(ROOT_INODE), AsRoot).unwrap();
        files.insert(ROOT_INODE, root_file);
        node_ids.insert(ROOT_INODE, root_node_id);

        FileManager {
            tree,
            files,
            node_ids,
            drive_ids: HashMap::new(),
            last_sync: SystemTime::now(),
            rename_identical_files,
            add_extensions_to_special_files: false,
            skip_trash: false,
            sync_interval: Duration::from_secs(60),
            df,
            last_inode: SHARED_INODE, // After ROOT=1, TRASH=2, SHARED=3
        }
    }

    #[cfg(test)]
    /// Manually add a file to the tree for testing (bypasses Drive API).
    /// This allows building test file structures without making network calls.
    pub fn add_test_file(&mut self, file: File, parent_inode: Inode) -> Result<(), Error> {
        let inode = file.inode();
        let drive_id = file.drive_id();

        // Add to files map
        self.files.insert(inode, file);

        // Create tree node
        let node = Node::new(inode);
        let parent_node_id = self
            .node_ids
            .get(&parent_inode)
            .ok_or_else(|| err_msg(format!("Parent inode {} not found", parent_inode)))?;

        let node_id = self.tree.insert(node, UnderNode(parent_node_id))?;

        // Update mappings
        self.node_ids.insert(inode, node_id);
        if let Some(id) = drive_id {
            self.drive_ids.insert(id, inode);
        }

        Ok(())
    }
}

impl fmt::Debug for FileManager {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "FileManager(")?;

        if self.tree.root_node_id().is_none() {
            return writeln!(f, ")");
        }

        let mut stack: Vec<(u32, &NodeId)> = vec![(0, self.tree.root_node_id().unwrap())];

        while let Some((level, node_id)) = stack.pop() {
            for _ in 0..level {
                write!(f, "\t")?;
            }

            let file = self.get_file(&FileId::NodeId(node_id.clone())).unwrap();
            writeln!(f, "{:3} => {}", file.inode(), file.name)?;

            self.tree.children_ids(node_id).unwrap().for_each(|id| {
                stack.push((level + 1, id));
            });
        }

        writeln!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::{FileManager, ROOT_INODE};
    use crate::gcsf::{Checksums, File, FileId};
    use fuser::{FileAttr, FileType, INodeNo};
    use std::time::SystemTime;

    fn file(name: &str, inode: u64, drive_id: &str, kind: FileType) -> File {
        File {
            name: name.to_string(),
            attr: FileAttr {
                ino: INodeNo(inode),
                size: 0,
                blocks: 0,
                atime: SystemTime::UNIX_EPOCH,
                mtime: SystemTime::UNIX_EPOCH,
                ctime: SystemTime::UNIX_EPOCH,
                crtime: SystemTime::UNIX_EPOCH,
                kind,
                perm: 0o755,
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: 0,
                blksize: 512,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive3::api::File {
                id: Some(drive_id.to_string()),
                name: Some(name.to_string()),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn deleting_directory_cleans_descendant_indexes() {
        let mut manager = FileManager::new_for_testing(false);
        manager
            .add_test_file(file("dir", 10, "dir-id", FileType::Directory), ROOT_INODE)
            .unwrap();
        manager
            .add_test_file(file("child", 11, "child-id", FileType::RegularFile), 10)
            .unwrap();

        manager.delete_locally(&FileId::Inode(10)).unwrap();

        for inode in [10, 11] {
            assert!(!manager.contains(&FileId::Inode(inode)));
            assert!(!manager.files.contains_key(&inode));
            assert!(!manager.node_ids.contains_key(&inode));
        }
        for drive_id in ["dir-id", "child-id"] {
            assert!(!manager.contains(&FileId::DriveId(drive_id.to_string())));
            assert!(!manager.drive_ids.contains_key(drive_id));
        }
    }

    #[test]
    fn deleting_duplicate_recalculates_survivor_suffix() {
        let mut manager = FileManager::new_for_testing(true);
        manager
            .add_test_file(file("same", 10, "a-id", FileType::RegularFile), ROOT_INODE)
            .unwrap();
        manager
            .add_test_file(file("same", 11, "b-id", FileType::RegularFile), ROOT_INODE)
            .unwrap();
        manager.recalculate_duplicate_suffixes_for_parent(ROOT_INODE);
        assert_eq!(
            manager.get_file(&FileId::Inode(11)).unwrap().name(),
            "same.1"
        );

        manager.delete_locally(&FileId::Inode(10)).unwrap();

        assert_eq!(manager.get_file(&FileId::Inode(11)).unwrap().name(), "same");
    }

    fn file_with_md5(name: &str, inode: u64, drive_id: &str, md5: &str) -> File {
        let mut file = file(name, inode, drive_id, FileType::RegularFile);
        let drive_file = file.drive_file.as_mut().unwrap();
        drive_file.md5_checksum = Some(md5.to_string());
        drive_file.sha256_checksum = Some(format!("sha256-of-{}", md5));
        file
    }

    fn md5_of(manager: &FileManager, inode: u64) -> Option<Option<String>> {
        manager
            .checksums(&FileId::Inode(inode))
            .map(|checksums| checksums.md5)
    }

    #[test]
    fn checksums_are_read_from_drive_metadata() {
        let mut manager = FileManager::new_for_testing(false);
        manager
            .add_test_file(file_with_md5("f", 10, "f-id", "listed"), ROOT_INODE)
            .unwrap();
        manager
            .add_test_file(file("d", 11, "d-id", FileType::Directory), ROOT_INODE)
            .unwrap();

        assert_eq!(md5_of(&manager, 10), Some(Some("listed".to_string())));
        assert_eq!(
            manager.checksums(&FileId::Inode(10)).unwrap().sha256,
            Some("sha256-of-listed".to_string())
        );
        assert_eq!(md5_of(&manager, 11), Some(None));
        assert_eq!(manager.checksums(&FileId::Inode(ROOT_INODE)), None);
        assert_eq!(manager.checksums(&FileId::Inode(99)), None);
    }

    #[test]
    fn checksums_are_hidden_while_writes_are_pending() {
        let mut manager = FileManager::new_for_testing(false);
        manager
            .add_test_file(file_with_md5("f", 10, "f-id", "listed"), ROOT_INODE)
            .unwrap();
        manager.write(&FileId::Inode(10), 0, b"changed").unwrap();
        assert_eq!(manager.checksums(&FileId::Inode(10)), None);
    }

    #[test]
    fn change_metadata_replaces_uploaded_checksums() {
        let mut manager = FileManager::new_for_testing(false);
        manager
            .add_test_file(file_with_md5("f", 10, "f-id", "stale"), ROOT_INODE)
            .unwrap();
        manager.df.record_uploaded_checksums_for_testing(
            "f-id",
            Checksums {
                md5: Some("uploaded".to_string()),
                sha256: None,
            },
        );
        assert_eq!(md5_of(&manager, 10), Some(Some("uploaded".to_string())));

        let changed = drive3::api::File {
            md5_checksum: Some("changed".to_string()),
            ..Default::default()
        };
        manager.adopt_checksums_from_change(&FileId::DriveId("f-id".to_string()), &changed);

        let checksums = manager.checksums(&FileId::Inode(10)).unwrap();
        assert_eq!(checksums.md5, Some("changed".to_string()));
        assert_eq!(checksums.sha256, None);
    }
}
