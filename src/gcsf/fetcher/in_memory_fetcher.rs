use super::DataFetcher;
use std::cmp;
use std::collections::HashMap;

type Inode = u64;

pub struct InMemoryFetcher {
    data: HashMap<Inode, Vec<u8>>,
}

impl DataFetcher for InMemoryFetcher {
    fn new() -> InMemoryFetcher {
        InMemoryFetcher {
            data: HashMap::new(),
        }
    }

    fn read(&mut self, inode: Inode, offset: usize, size: usize) -> Option<&[u8]> {
        self.data
            .get(&inode)
            .map(|data| &data[offset..cmp::min(data.len(), offset + size)])
    }

    fn write(&mut self, inode: Inode, offset: usize, data: &[u8]) {
        if !self.data.contains_key(&inode) {
            self.data.insert(inode, Vec::new());
        }

        let file_data: &mut Vec<u8> = self.data.get_mut(&inode).unwrap();
        let old_size = file_data.len();
        let new_size = offset + data.len();

        file_data.resize(new_size, 0);
        if new_size < old_size {
            file_data.shrink_to_fit();
        }

        file_data[offset..].copy_from_slice(&data[..]);
    }

    fn remove(&mut self, inode: Inode) {
        self.data.remove(&inode);
    }
}
