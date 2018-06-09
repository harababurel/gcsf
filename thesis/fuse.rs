pub trait Filesystem {
    /// Initialize filesystem.
    fn init(&mut self, _req: &Request) -> Result<(), c_int>;

    /// Clean up filesystem.
    fn destroy(&mut self, _req: &Request);

    /// Look up a directory entry by name and get its attributes.
    fn lookup(&mut self, _req: &Request, _parent: u64, _name: &OsStr, reply: ReplyEntry);

    /// Forget about an inode.
    fn forget(&mut self, _req: &Request, _ino: u64, _nlookup: u64);

    /// Get file attributes.
    fn getattr(&mut self, _req: &Request, _ino: u64, reply: ReplyAttr);

    /// Set file attributes.
    fn setattr(
        &mut self,
        _req: &Request,
        _ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<Timespec>,
        _mtime: Option<Timespec>,
        _fh: Option<u64>,
        _crtime: Option<Timespec>,
        _chgtime: Option<Timespec>,
        _bkuptime: Option<Timespec>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    );

    /// Read symbolic link.
    fn readlink(&mut self, _req: &Request, _ino: u64, reply: ReplyData);

    /// Create file node.
    fn mknod(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _mode: u32,
        _rdev: u32,
        reply: ReplyEntry,
    );

    /// Create a directory.
    fn mkdir(&mut self, _req: &Request, _parent: u64, _name: &OsStr, _mode: u32, reply: ReplyEntry);

    /// Remove a file.
    fn unlink(&mut self, _req: &Request, _parent: u64, _name: &OsStr, reply: ReplyEmpty);

    /// Remove a directory.
    fn rmdir(&mut self, _req: &Request, _parent: u64, _name: &OsStr, reply: ReplyEmpty);

    /// Create a symbolic link.
    fn symlink(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _link: &Path,
        reply: ReplyEntry,
    );

    /// Rename a file.
    fn rename(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _newparent: u64,
        _newname: &OsStr,
        reply: ReplyEmpty,
    );

    /// Create a hard link.
    fn link(
        &mut self,
        _req: &Request,
        _ino: u64,
        _newparent: u64,
        _newname: &OsStr,
        reply: ReplyEntry,
    );

    /// Open a file.
    fn open(&mut self, _req: &Request, _ino: u64, _flags: u32, reply: ReplyOpen);

    /// Read data.
    fn read(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _offset: i64,
        _size: u32,
        reply: ReplyData,
    );

    /// Write data.
    fn write(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _offset: i64,
        _data: &[u8],
        _flags: u32,
        reply: ReplyWrite,
    );

    /// Flush method.
    fn flush(&mut self, _req: &Request, _ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty);

    /// Release an open file.
    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
        reply: ReplyEmpty,
    );

    /// Synchronize file contents.
    fn fsync(&mut self, _req: &Request, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty);

    /// Open a directory.
    fn opendir(&mut self, _req: &Request, _ino: u64, _flags: u32, reply: ReplyOpen);

    /// Read directory.
    fn readdir(&mut self, _req: &Request, _ino: u64, _fh: u64, _offset: i64, reply: ReplyDirectory);

    /// Release an open directory.
    fn releasedir(&mut self, _req: &Request, _ino: u64, _fh: u64, _flags: u32, reply: ReplyEmpty);

    /// Synchronize directory contents.
    fn fsyncdir(&mut self, _req: &Request, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty);

    /// Get file system statistics.
    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs);

    /// Set an extended attribute.
    fn setxattr(
        &mut self,
        _req: &Request,
        _ino: u64,
        _name: &OsStr,
        _value: &[u8],
        _flags: u32,
        _position: u32,
        reply: ReplyEmpty,
    );

    /// Get an extended attribute.
    fn getxattr(&mut self, _req: &Request, _ino: u64, _name: &OsStr, _size: u32, reply: ReplyXattr);

    /// List extended attribute names.
    fn listxattr(&mut self, _req: &Request, _ino: u64, _size: u32, reply: ReplyXattr);

    /// Remove an extended attribute.
    fn removexattr(&mut self, _req: &Request, _ino: u64, _name: &OsStr, reply: ReplyEmpty);

    /// Check file access permissions.
    fn access(&mut self, _req: &Request, _ino: u64, _mask: u32, reply: ReplyEmpty);

    /// Create and open a file.
    fn create(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _mode: u32,
        _flags: u32,
        reply: ReplyCreate,
    );

    /// Test for a POSIX file lock.
    fn getlk(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        _start: u64,
        _end: u64,
        _typ: u32,
        _pid: u32,
        reply: ReplyLock,
    );

    /// Acquire, modify or release a POSIX file lock.
    fn setlk(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        _start: u64,
        _end: u64,
        _typ: u32,
        _pid: u32,
        _sleep: bool,
        reply: ReplyEmpty,
    );

    /// Map block index within file to block index within device.
    fn bmap(&mut self, _req: &Request, _ino: u64, _blocksize: u32, _idx: u64, reply: ReplyBmap);
}
