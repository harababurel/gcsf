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
    reply: ReplyAttr
) { ... }
