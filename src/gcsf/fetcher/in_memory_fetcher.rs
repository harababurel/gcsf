use super::DataFetcher;
use std::collections::HashMap;

pub struct InMemoryFetcher {
    data: HashMap<String, Vec<u8>>,
}

impl DataFetcher for InMemoryFetcher {
    fn new() -> InMemoryFetcher {
        InMemoryFetcher {
            data: HashMap::new(),
        }
    }

    fn get_data(&self, piece_name: &str) -> Option<&Vec<u8>> {
        self.data.get(piece_name)
    }
}
