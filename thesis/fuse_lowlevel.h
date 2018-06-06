struct fuse_lowlevel_ops {
  // Initialize filesystem
  void (*init)(void *userdata, struct fuse_conn_info *conn);

  // Clean up filesystem.
  void (*destroy)(void *userdata);

  // Look up a directory entry by name and get its attributes.
  void (*lookup)(fuse_req_t req, fuse_ino_t parent, const char *name);

  // Forget about an inode
  void (*forget)(fuse_req_t req, fuse_ino_t ino, uint64_t nlookup);

  // Get file attributes.
  void (*getattr)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi);

  // Set file attributes
  void (*setattr)(fuse_req_t req, fuse_ino_t ino, struct stat *attr, int to_set,
                  struct fuse_file_info *fi);

  // Read symbolic link
  void (*readlink)(fuse_req_t req, fuse_ino_t ino);

  // Create file node
  void (*mknod)(fuse_req_t req, fuse_ino_t parent, const char *name,
                mode_t mode, dev_t rdev);

  // Create a directory
  void (*mkdir)(fuse_req_t req, fuse_ino_t parent, const char *name,
                mode_t mode);

  // Remove a file
  void (*unlink)(fuse_req_t req, fuse_ino_t parent, const char *name);

  // Remove a directory
  void (*rmdir)(fuse_req_t req, fuse_ino_t parent, const char *name);

  // Create a symbolic link
  void (*symlink)(fuse_req_t req, const char *link, fuse_ino_t parent,
                  const char *name);

  // Rename a file
  void (*rename)(fuse_req_t req, fuse_ino_t parent, const char *name,
                 fuse_ino_t newparent, const char *newname, unsigned int flags);

  // Create a hard link
  void (*link)(fuse_req_t req, fuse_ino_t ino, fuse_ino_t newparent,
               const char *newname);

  // Open a file
  void (*open)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi);

  // Read data
  void (*read)(fuse_req_t req, fuse_ino_t ino, size_t size, off_t off,
               struct fuse_file_info *fi);

  // Write data
  void (*write)(fuse_req_t req, fuse_ino_t ino, const char *buf, size_t size,
                off_t off, struct fuse_file_info *fi);

  // Flush method
  void (*flush)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi);

  // Release an open file
  void (*release)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi);

  // Synchronize file contents
  void (*fsync)(fuse_req_t req, fuse_ino_t ino, int datasync,
                struct fuse_file_info *fi);

  // Open a directory
  void (*opendir)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi);

  // Read directory
  void (*readdir)(fuse_req_t req, fuse_ino_t ino, size_t size, off_t off,
                  struct fuse_file_info *fi);

  // Release an open directory
  void (*releasedir)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi);

  // Synchronize directory contents
  void (*fsyncdir)(fuse_req_t req, fuse_ino_t ino, int datasync,
                   struct fuse_file_info *fi);

  // Get file system statistics
  void (*statfs)(fuse_req_t req, fuse_ino_t ino);

  // Set an extended attribute
  void (*setxattr)(fuse_req_t req, fuse_ino_t ino, const char *name,
                   const char *value, size_t size, int flags);

  // Get an extended attribute
  void (*getxattr)(fuse_req_t req, fuse_ino_t ino, const char *name,
                   size_t size);

  // List extended attribute names
  void (*listxattr)(fuse_req_t req, fuse_ino_t ino, size_t size);

  // Remove an extended attribute
  void (*removexattr)(fuse_req_t req, fuse_ino_t ino, const char *name);

  // Check file access permissions
  void (*access)(fuse_req_t req, fuse_ino_t ino, int mask);

  // Create and open a file
  void (*create)(fuse_req_t req, fuse_ino_t parent, const char *name,
                 mode_t mode, struct fuse_file_info *fi);

  // Test for a POSIX file lock
  void (*getlk)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi,
                struct flock *lock);

  // Acquire, modify or release a POSIX file lock
  void (*setlk)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi,
                struct flock *lock, int sleep);

  // Map block index within file to block index within device
  void (*bmap)(fuse_req_t req, fuse_ino_t ino, size_t blocksize, uint64_t idx);

  // Ioctl
  void (*ioctl)(fuse_req_t req, fuse_ino_t ino, int cmd, void *arg,
                struct fuse_file_info *fi, unsigned flags, const void *in_buf,
                size_t in_bufsz, size_t out_bufsz);

  // Poll for IO readiness
  void (*poll)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi,
               struct fuse_pollhandle *ph);

  // Write data made available in a buffer
  void (*write_buf)(fuse_req_t req, fuse_ino_t ino, struct fuse_bufvec *bufv,
                    off_t off, struct fuse_file_info *fi);

  // Callback function for the retrieve request
  void (*retrieve_reply)(fuse_req_t req, void *cookie, fuse_ino_t ino,
                         off_t offset, struct fuse_bufvec *bufv);

  // Forget about multiple inodes
  void (*forget_multi)(fuse_req_t req, size_t count,
                       struct fuse_forget_data *forgets);

  // Acquire, modify or release a BSD file lock
  void (*flock)(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info *fi,
                int op);

  // Allocate requested space. If this function returns success then
  void (*fallocate)(fuse_req_t req, fuse_ino_t ino, int mode, off_t offset,
                    off_t length, struct fuse_file_info *fi);

  // Read directory with attributes
  void (*readdirplus)(fuse_req_t req, fuse_ino_t ino, size_t size, off_t off,
                      struct fuse_file_info *fi);
};
