type Inode = u64;

pub trait DataFetcher {
    fn new() -> Self;
    fn read(&self, inode: Inode, offset: usize, size: usize) -> Option<&[u8]>;
    fn write(&mut self, inode: Inode, offset: usize, data: &[u8]);
}
