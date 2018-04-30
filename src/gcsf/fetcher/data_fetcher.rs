pub trait DataFetcher {
    fn new() -> Self;
    fn get_data(&self, piece_name: &str) -> Option<&Vec<u8>>;
}
