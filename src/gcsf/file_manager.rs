use super::{File, FileId};
use id_tree::InsertBehavior::*;
use id_tree::MoveBehavior::*;
use id_tree::RemoveBehavior::*;
use id_tree::{Node, NodeId, NodeIdError, Tree, TreeBuilder};
use std::collections::HashMap;
use std::fmt;

pub type Inode = u64;
pub type DriveId = String;

pub struct FileManager {
    tree: Tree<Inode>,
    pub files: HashMap<Inode, File>,
    pub node_ids: HashMap<Inode, NodeId>,
    pub drive_ids: HashMap<DriveId, Inode>,
}

impl FileManager {
    pub fn new() -> Self {
        FileManager {
            tree: TreeBuilder::new().with_node_capacity(500).build(),
            files: HashMap::new(),
            node_ids: HashMap::new(),
            drive_ids: HashMap::new(),
        }
    }

    pub fn contains(&self, file_id: FileId) -> bool {
        match file_id {
            FileId::Inode(inode) => self.node_ids.contains_key(&inode),
            FileId::DriveId(drive_id) => self.drive_ids.contains_key(&drive_id),
            FileId::NodeId(node_id) => self.tree.get(&node_id).is_ok(),
            FileId::ParentAndName { parent, name } => {
                self.get_file(FileId::ParentAndName { parent, name })
                    .is_some()
            }
        }
    }

    pub fn get_node_id(&self, file_id: FileId) -> Option<NodeId> {
        match file_id {
            FileId::Inode(inode) => self.node_ids.get(&inode).cloned(),
            FileId::DriveId(drive_id) => self.get_node_id(FileId::Inode(self.get_inode(
                FileId::DriveId(drive_id),
            ).unwrap())),
            FileId::NodeId(node_id) => Some(node_id),
            FileId::ParentAndName { parent, name } => {
                let inode = self.get_inode(FileId::ParentAndName { parent, name })?;
                self.get_node_id(FileId::Inode(inode))
            }
        }
    }

    pub fn get_drive_id(&self, id: FileId) -> Option<DriveId> {
        self.get_file(id)?.drive_id()
    }

    pub fn get_inode(&self, id: FileId) -> Option<Inode> {
        // debug!("get_inode({:?})", &id);
        match id {
            FileId::Inode(inode) => Some(inode),
            FileId::DriveId(drive_id) => self.drive_ids.get(&drive_id).cloned(),
            FileId::NodeId(node_id) => self.tree
                .get(&node_id)
                .map(|node| node.data())
                .ok()
                .cloned(),
            FileId::ParentAndName { parent, name } => self.get_children(FileId::Inode(parent))?
                .into_iter()
                .find(|child| child.name == name)
                .map(|child| child.inode()),
        }
    }

    pub fn get_children(&self, id: FileId) -> Option<Vec<&File>> {
        // debug!("get_children({:?})", &id);
        let node_id = self.get_node_id(id)?;
        let children: Vec<&File> = self.tree
            .children(&node_id)
            .unwrap()
            .map(|child| self.get_file(FileId::Inode(*child.data())))
            .filter(Option::is_some)
            .map(Option::unwrap)
            .collect();

        Some(children)
    }

    pub fn get_file(&self, id: FileId) -> Option<&File> {
        // debug!("get_file({:?})", &id);
        let inode = self.get_inode(id)?;
        self.files.get(&inode)
    }

    pub fn add_file(&mut self, file: File, parent: Option<FileId>) {
        let node_id = match parent {
            Some(inode) => {
                let parent_id = self.get_node_id(inode).unwrap();
                self.tree
                    .insert(Node::new(file.inode()), UnderNode(&parent_id))
                    .unwrap()
            }
            None => {
                error!("Adding as root!!!");
                self.tree.insert(Node::new(file.inode()), AsRoot).unwrap()
            }
        };

        self.node_ids.insert(file.inode(), node_id);
        file.drive_id()
            .and_then(|drive_id| self.drive_ids.insert(drive_id, file.inode()));
        self.files.insert(file.inode(), file);
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

            let file = self.get_file(FileId::NodeId(node_id.clone())).unwrap();
            write!(f, "{:3} => {}\n", file.inode(), file.name)?;

            self.tree.children_ids(node_id).unwrap().for_each(|id| {
                stack.push((level + 1, id));
            });
        }

        write!(f, ")\n")
    }
}
