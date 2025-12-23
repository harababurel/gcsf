# GCSF Integration Tests

Comprehensive end-to-end tests that validate all common filesystem operations on a mounted GCSF instance.

## Prerequisites

1. **Build GCSF**
   ```bash
   cargo build --release
   ```

2. **Set up authentication**
   ```bash
   cargo run --release -- login test_session
   ```

3. **Configure GCSF**

   Edit `~/.config/gcsf/gcsf.toml`:
   ```toml
   rename_identical_files = true
   sync_interval = 5
   ```

   Note: `rename_identical_files = true` is required for tests 12-14 to pass correctly.

## Running the Tests

```bash
# Terminal 1: Mount GCSF
mkdir -p ~/gcsf_test_mount
cargo run --release -- mount ~/gcsf_test_mount -s test_session

# Terminal 2: Run tests
cd tests/integration
./test_filesystem_operations.sh --accept-risk ~/gcsf_test_mount

# Terminal 1: Unmount when done
fusermount -u ~/gcsf_test_mount
# Or press Ctrl+C in the mount terminal
```

## What Gets Tested

The test suite includes **14 test categories** covering all typical filesystem operations:

1. **File Creation** - touch, echo, printf, heredoc
2. **File Writing** - overwrite, append, multiple writes
3. **File Reading** - cat, head, tail, wc, grep
4. **File Moving & Renaming** - mv within/across directories
5. **File Deletion** - single files, wildcards, selective deletion
6. **Directory Operations** - mkdir, nested dirs, rmdir, rm -rf
7. **Large Files** - 1MB and 10MB file handling
8. **Special Characters** - spaces, dots, parentheses in filenames
9. **Content Integrity** - ASCII, Unicode, binary data preservation
10. **Concurrent Operations** - parallel file creation
11. **Copy Operations** - cp, cp -r for files and directories
12. **Duplicate Handling - Same Directory** - unique files don't get suffixes
13. **Same Filename, Different Directories** - files in different dirs don't get suffixes
14. **Nested Directories** - same filename in nested paths behaves correctly

## Expected Output

```
================================================
  GCSF Comprehensive Filesystem Operations Test
================================================

=== TEST 1: Basic file creation ===
✓ PASS: Empty file created with touch
✓ PASS: File created with echo redirect
✓ PASS: Echo redirect content
...

=== TEST 14: Same filename in nested directories ===
✓ PASS: data.txt in a/b/c
✓ PASS: data.txt in a/b/d
✓ PASS: data.txt in x/y/z
...

================================================
  Test Summary
================================================
Tests run:    14
Tests passed: 60
Tests failed: 0
Pass rate:    100%

✓✓✓ All tests passed! GCSF is working correctly ✓✓✓
```

## Troubleshooting

### "Mount point does not exist"
```bash
mkdir -p ~/gcsf_test_mount
```

### "Permission denied"
```bash
chmod +x ./test_filesystem_operations.sh
```

### "Authentication failed"
```bash
cargo run --release -- logout test_session
cargo run --release -- login test_session
```

### Tests fail with "file does not exist"
- Verify mount is active: `mount | grep gcsf`
- Check config has `rename_identical_files = true`
- Try increasing `sync_interval` in config if operations are too fast

### Mount is slow or unresponsive
- Check network connection
- Enable debug logging: set `debug = true` in config
- Check Google Drive API quota/rate limits

## Cleanup

The test script automatically cleans up on exit. If cleanup fails:

```bash
rm -rf ~/gcsf_test_mount/gcsf_ops_test_*
```

## Notes

- Tests run in an isolated directory with timestamp to avoid conflicts
- All tests continue even if some fail
- Total runtime: typically <10 minutes depending on network speed
- The `--accept-risk` flag is required as tests modify your Google Drive
