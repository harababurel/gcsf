type Inode = u64;

pub trait DataFetcher {
    fn new() -> Self;
    fn read(&mut self, inode: Inode, offset: usize, size: usize) -> Option<&[u8]>;
    fn write(&mut self, inode: Inode, offset: usize, data: &[u8]);
    fn remove(&mut self, inode: Inode);
    fn flush(&mut self, inode: Inode) {
        warn!(
            "DataFetcher::flush(inode={}) called, but no trait implementation is provided",
            inode
        );
    }
    fn size_and_capacity(&mut self) -> (u64, Option<u64>) {
        warn!("DataFetcher::size_and_capacity() called, but no trait implementation is provided");
        (0, Some(0))
    }
}
