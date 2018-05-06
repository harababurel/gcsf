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
}
