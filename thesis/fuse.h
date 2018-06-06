struct fuse_operations {
  // Get file attributes.
  int (*getattr)(const char *, struct stat *, struct fuse_file_info *fi);

  // Read the target of a symbolic link
  int (*readlink)(const char *, char *, size_t);

  // Create a file node
  int (*mknod)(const char *, mode_t, dev_t);

  // Create a directory
  int (*mkdir)(const char *, mode_t);

  // Remove a file
  int (*unlink)(const char *);

  // Remove a directory
  int (*rmdir)(const char *);

  // Create a symbolic link
  int (*symlink)(const char *, const char *);

  // Rename a file
  int (*rename)(const char *, const char *, unsigned int flags);

  // Create a hard link to a file
  int (*link)(const char *, const char *);

  // Change the permission bits of a file
  int (*chmod)(const char *, mode_t, struct fuse_file_info *fi);

  // Change the owner and group of a file
  int (*chown)(const char *, uid_t, gid_t, struct fuse_file_info *fi);

  // Change the size of a file
  int (*truncate)(const char *, off_t, struct fuse_file_info *fi);

  // Open a file
  int (*open)(const char *, struct fuse_file_info *);

  // Read data from an open file
  int (*read)(const char *, char *, size_t, off_t, struct fuse_file_info *);

  // Write data to an open file
  int (*write)(const char *, const char *, size_t, off_t,
               struct fuse_file_info *);

  // Get file system statistics
  int (*statfs)(const char *, struct statvfs *);

  // Possibly flush cached data
  int (*flush)(const char *, struct fuse_file_info *);

  // Release an open file
  int (*release)(const char *, struct fuse_file_info *);

  // Synchronize file contents
  int (*fsync)(const char *, int, struct fuse_file_info *);

  // Set extended attributes
  int (*setxattr)(const char *, const char *, const char *, size_t, int);

  // Get extended attributes
  int (*getxattr)(const char *, const char *, char *, size_t);

  // List extended attributes
  int (*listxattr)(const char *, char *, size_t);

  // Remove extended attributes
  int (*removexattr)(const char *, const char *);

  // Open directory
  int (*opendir)(const char *, struct fuse_file_info *);

  // Read directory
  int (*readdir)(const char *, void *, fuse_fill_dir_t, off_t,
                 struct fuse_file_info *, enum fuse_readdir_flags);

  // Release directory
  int (*releasedir)(const char *, struct fuse_file_info *);

  // Synchronize directory contents
  int (*fsyncdir)(const char *, int, struct fuse_file_info *);

  // Initialize filesystem
  void *(*init)(struct fuse_conn_info *conn, struct fuse_config *cfg);

  // Clean up filesystem
  void (*destroy)(void *private_data);

  // Check file access permissions
  int (*access)(const char *, int);

  // Create and open a file
  int (*create)(const char *, mode_t, struct fuse_file_info *);

  // Perform POSIX file locking operation
  int (*lock)(const char *, struct fuse_file_info *, int cmd, struct flock *);

  // Change the access and modification times of a file with
  int (*utimens)(const char *, const struct timespec tv[2],
                 struct fuse_file_info *fi);

  // Map block index within file to block index within device
  int (*bmap)(const char *, size_t blocksize, uint64_t *idx);

  // Ioctl
  int (*ioctl)(const char *, int cmd, void *arg, struct fuse_file_info *,
               unsigned int flags, void *data);

  // Poll for IO readiness events
  int (*poll)(const char *, struct fuse_file_info *, struct fuse_pollhandle *ph,
              unsigned *reventsp);

  // Write contents of buffer to an open file
  int (*write_buf)(const char *, struct fuse_bufvec *buf, off_t off,
                   struct fuse_file_info *);

  // Store data from an open file in a buffer
  int (*read_buf)(const char *, struct fuse_bufvec **bufp, size_t size,
                  off_t off, struct fuse_file_info *);

  // Perform BSD file locking operation
  int (*flock)(const char *, struct fuse_file_info *, int op);

  // Allocates space for an open file
  int (*fallocate)(const char *, int, off_t, off_t, struct fuse_file_info *);
};
