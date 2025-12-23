#!/bin/bash
#
# Comprehensive GCSF Filesystem Operations Test Suite
#
# Tests all common filesystem operations that a typical user would perform:
# - Creating files and directories
# - Writing to files (new, overwrite, append)
# - Reading from files
# - Moving and renaming files
# - Deleting files and directories
# - Validating file contents and metadata
# - Edge cases and error conditions
#
# REQUIREMENTS:
# - gcsf must be mounted at the specified mount point
# - The test directory should start empty
#
# USAGE:
#   ./test_filesystem_operations.sh <mount_point>
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Check for safety flag
ACCEPT_RISK=false
MOUNT_POINT=""

for arg in "$@"; do
    case $arg in
        --accept-risk)
            ACCEPT_RISK=true
            shift
            ;;
        *)
            MOUNT_POINT="$arg"
            ;;
    esac
done

# Check arguments
if [ -z "$MOUNT_POINT" ]; then
    echo "Usage: $0 [--accept-risk] <mount_point>"
    echo "Example: $0 --accept-risk ~/gcsf_mount"
    exit 1
fi

# Safety check - require explicit confirmation
if [ "$ACCEPT_RISK" = false ]; then
    echo -e "${YELLOW}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${YELLOW}║${NC}  ${RED}⚠ WARNING: This test will modify your Google Drive${NC}          ${YELLOW}║${NC}"
    echo -e "${YELLOW}╚═══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "${CYAN}This test script will:${NC}"
    echo "  • Create files and directories in your mounted Google Drive"
    echo "  • Write, modify, and delete test data"
    echo "  • Move files between directories"
    echo "  • Create files up to 10MB in size"
    echo ""
    echo -e "${CYAN}Mount point:${NC} $MOUNT_POINT"
    echo -e "${CYAN}Test directory:${NC} $MOUNT_POINT/gcsf_integration_test_*"
    echo ""
    echo -e "${YELLOW}The test creates an isolated directory and cleans up after itself,${NC}"
    echo -e "${YELLOW}but you should understand the risks before proceeding.${NC}"
    echo ""
    echo -e "${GREEN}To run this test, add the --accept-risk flag:${NC}"
    echo -e "  $0 ${GREEN}--accept-risk${NC} $MOUNT_POINT"
    echo ""
    exit 1
fi

# Validate mount point exists
if [ ! -d "$MOUNT_POINT" ]; then
    echo -e "${RED}ERROR: Mount point '$MOUNT_POINT' does not exist${NC}"
    exit 1
fi

# Create test directory with timestamp
TEST_DIR="$MOUNT_POINT/gcsf_ops_test_$(date +%s)"

# Cleanup function
cleanup() {
    echo ""
    echo -e "${BLUE}=== Cleanup ===${NC}"
    if [ -d "$TEST_DIR" ]; then
        echo "Removing test directory: $TEST_DIR"
        rm -rf "$TEST_DIR" 2>/dev/null || true
    fi
}

trap cleanup EXIT

# Logging functions
log_test() {
    echo ""
    echo -e "${BLUE}=== TEST $1: $2 ===${NC}"
    TESTS_RUN=$((TESTS_RUN + 1))
}

log_pass() {
    echo -e "${GREEN}✓ PASS${NC}: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

log_fail() {
    echo -e "${RED}✗ FAIL${NC}: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

log_info() {
    echo -e "${CYAN}ℹ${NC} $1"
}

# Wait for filesystem sync
wait_for_sync() {
    sleep 0.2
}

# Assertion helpers
assert_file_exists() {
    local filepath="$1"
    local description="$2"

    if [ -f "$filepath" ]; then
        log_pass "$description"
        return 0
    else
        log_fail "$description - file not found: $filepath"
        return 1
    fi
}

assert_file_not_exists() {
    local filepath="$1"
    local description="$2"

    if [ ! -f "$filepath" ]; then
        log_pass "$description"
        return 0
    else
        log_fail "$description - file should not exist: $filepath"
        return 1
    fi
}

assert_dir_exists() {
    local dirpath="$1"
    local description="$2"

    if [ -d "$dirpath" ]; then
        log_pass "$description"
        return 0
    else
        log_fail "$description - directory not found: $dirpath"
        return 1
    fi
}

assert_content_equals() {
    local filepath="$1"
    local expected="$2"
    local description="$3"

    if [ ! -f "$filepath" ]; then
        log_fail "$description - file not found: $filepath"
        return 1
    fi

    local actual
    actual=$(cat "$filepath")

    if [ "$actual" = "$expected" ]; then
        log_pass "$description"
        return 0
    else
        log_fail "$description"
        echo "  Expected: '$expected'"
        echo "  Got:      '$actual'"
        return 1
    fi
}

assert_content_contains() {
    local filepath="$1"
    local substring="$2"
    local description="$3"

    if [ ! -f "$filepath" ]; then
        log_fail "$description - file not found: $filepath"
        return 1
    fi

    if grep -q "$substring" "$filepath"; then
        log_pass "$description"
        return 0
    else
        log_fail "$description - substring not found: '$substring'"
        echo "  File contents:"
        cat "$filepath"
        return 1
    fi
}

assert_file_size_greater() {
    local filepath="$1"
    local min_size="$2"
    local description="$3"

    if [ ! -f "$filepath" ]; then
        log_fail "$description - file not found: $filepath"
        return 1
    fi

    local actual_size
    actual_size=$(stat -c%s "$filepath" 2>/dev/null || stat -f%z "$filepath" 2>/dev/null)

    if [ "$actual_size" -gt "$min_size" ]; then
        log_pass "$description (size: $actual_size bytes)"
        return 0
    else
        log_fail "$description - expected > $min_size bytes, got $actual_size"
        return 1
    fi
}

#############################################################################
# TEST 1: Basic File Creation
#############################################################################
test_file_creation() {
    log_test "1" "Basic file creation"

    local test_dir="$TEST_DIR/test1"
    mkdir -p "$test_dir"

    # Method 1: touch
    log_info "Creating empty file with touch"
    touch "$test_dir/empty.txt"
    wait_for_sync
    assert_file_exists "$test_dir/empty.txt" "Empty file created with touch"

    # Method 2: echo redirect
    log_info "Creating file with echo redirect"
    echo "Hello, World!" > "$test_dir/hello.txt"
    wait_for_sync
    assert_file_exists "$test_dir/hello.txt" "File created with echo redirect"
    assert_content_equals "$test_dir/hello.txt" "Hello, World!" "Echo redirect content"

    # Method 3: printf
    log_info "Creating file with printf"
    printf "Line 1\nLine 2\nLine 3\n" > "$test_dir/multiline.txt"
    wait_for_sync
    assert_file_exists "$test_dir/multiline.txt" "File created with printf"
    assert_content_contains "$test_dir/multiline.txt" "Line 2" "Multiline content"

    # Method 4: cat with heredoc
    log_info "Creating file with cat heredoc"
    cat > "$test_dir/heredoc.txt" << 'EOF'
This is a heredoc
with multiple lines
and special characters: !@#$%
EOF
    wait_for_sync
    assert_file_exists "$test_dir/heredoc.txt" "File created with heredoc"
    assert_content_contains "$test_dir/heredoc.txt" "special characters" "Heredoc content"
}

#############################################################################
# TEST 2: Writing to Files
#############################################################################
test_file_writing() {
    log_test "2" "Writing to files (overwrite and append)"

    local test_dir="$TEST_DIR/test2"
    mkdir -p "$test_dir"

    # Create initial file
    echo "Initial content" > "$test_dir/data.txt"
    wait_for_sync
    assert_content_equals "$test_dir/data.txt" "Initial content" "Initial file content"

    # Overwrite
    log_info "Overwriting file"
    echo "Overwritten content" > "$test_dir/data.txt"
    wait_for_sync
    assert_content_equals "$test_dir/data.txt" "Overwritten content" "Overwritten content"

    # Append
    log_info "Appending to file"
    echo "Appended line" >> "$test_dir/data.txt"
    wait_for_sync
    assert_content_contains "$test_dir/data.txt" "Overwritten content" "Original content still present"
    assert_content_contains "$test_dir/data.txt" "Appended line" "Appended content present"

    # Multiple appends
    log_info "Multiple appends"
    echo "Line 2" >> "$test_dir/data.txt"
    echo "Line 3" >> "$test_dir/data.txt"
    wait_for_sync
    local line_count
    line_count=$(wc -l < "$test_dir/data.txt")
    if [ "$line_count" -eq 4 ]; then
        log_pass "Multiple appends resulted in 4 lines"
    else
        log_fail "Expected 4 lines, got $line_count"
    fi
}

#############################################################################
# TEST 3: Reading from Files
#############################################################################
test_file_reading() {
    log_test "3" "Reading from files"

    local test_dir="$TEST_DIR/test3"
    mkdir -p "$test_dir"

    # Create test file
    cat > "$test_dir/test.txt" << 'EOF'
Line 1
Line 2
Line 3
Line 4
Line 5
EOF
    wait_for_sync

    # cat
    log_info "Reading entire file with cat"
    if cat "$test_dir/test.txt" | grep -q "Line 3"; then
        log_pass "cat successfully read file"
    else
        log_fail "cat did not read file correctly"
    fi

    # head
    log_info "Reading first lines with head"
    local first_line
    first_line=$(head -n 1 "$test_dir/test.txt")
    if [ "$first_line" = "Line 1" ]; then
        log_pass "head -n 1 read first line"
    else
        log_fail "head -n 1 failed (got: '$first_line')"
    fi

    # tail
    log_info "Reading last lines with tail"
    local last_line
    last_line=$(tail -n 1 "$test_dir/test.txt")
    if [ "$last_line" = "Line 5" ]; then
        log_pass "tail -n 1 read last line"
    else
        log_fail "tail -n 1 failed (got: '$last_line')"
    fi

    # wc
    log_info "Counting lines with wc"
    local line_count
    line_count=$(wc -l < "$test_dir/test.txt")
    if [ "$line_count" -eq 5 ]; then
        log_pass "wc -l counted 5 lines"
    else
        log_fail "wc -l failed (got: $line_count)"
    fi

    # grep
    log_info "Searching with grep"
    if grep -q "Line 3" "$test_dir/test.txt"; then
        log_pass "grep found 'Line 3'"
    else
        log_fail "grep did not find 'Line 3'"
    fi
}

#############################################################################
# TEST 4: File Moving and Renaming
#############################################################################
test_file_moving() {
    log_test "4" "Moving and renaming files"

    local test_dir="$TEST_DIR/test4"
    mkdir -p "$test_dir/source"
    mkdir -p "$test_dir/dest"

    # Create source file
    echo "movable content" > "$test_dir/source/original.txt"
    wait_for_sync

    # Rename within same directory
    log_info "Renaming file in same directory"
    mv "$test_dir/source/original.txt" "$test_dir/source/renamed.txt"
    wait_for_sync
    assert_file_not_exists "$test_dir/source/original.txt" "Original file removed after rename"
    assert_file_exists "$test_dir/source/renamed.txt" "Renamed file exists"
    assert_content_equals "$test_dir/source/renamed.txt" "movable content" "Content preserved after rename"

    # Move to different directory
    log_info "Moving file to different directory"
    mv "$test_dir/source/renamed.txt" "$test_dir/dest/moved.txt"
    wait_for_sync
    assert_file_not_exists "$test_dir/source/renamed.txt" "File removed from source"
    assert_file_exists "$test_dir/dest/moved.txt" "File exists in destination"
    assert_content_equals "$test_dir/dest/moved.txt" "movable content" "Content preserved after move"

    # Move and rename simultaneously
    log_info "Move and rename simultaneously"
    echo "combo content" > "$test_dir/source/file1.txt"
    wait_for_sync
    mv "$test_dir/source/file1.txt" "$test_dir/dest/file2.txt"
    wait_for_sync
    assert_file_not_exists "$test_dir/source/file1.txt" "Source file removed"
    assert_file_exists "$test_dir/dest/file2.txt" "Destination file exists with new name"
    assert_content_equals "$test_dir/dest/file2.txt" "combo content" "Content preserved"
}

#############################################################################
# TEST 5: File Deletion
#############################################################################
test_file_deletion() {
    log_test "5" "Deleting files"

    local test_dir="$TEST_DIR/test5"
    mkdir -p "$test_dir"

    # Create and delete single file
    log_info "Creating and deleting single file"
    echo "temporary" > "$test_dir/temp.txt"
    wait_for_sync
    assert_file_exists "$test_dir/temp.txt" "File created"

    rm "$test_dir/temp.txt"
    wait_for_sync
    assert_file_not_exists "$test_dir/temp.txt" "File deleted"

    # Create multiple files and delete selectively
    log_info "Selective deletion"
    echo "keep" > "$test_dir/keep1.txt"
    echo "keep" > "$test_dir/keep2.txt"
    echo "delete" > "$test_dir/delete.txt"
    wait_for_sync

    rm "$test_dir/delete.txt"
    wait_for_sync

    assert_file_exists "$test_dir/keep1.txt" "Keep1 still exists"
    assert_file_exists "$test_dir/keep2.txt" "Keep2 still exists"
    assert_file_not_exists "$test_dir/delete.txt" "Delete file removed"

    # Delete with wildcard
    log_info "Deleting with wildcard pattern"
    echo "x" > "$test_dir/remove1.txt"
    echo "x" > "$test_dir/remove2.txt"
    echo "x" > "$test_dir/remove3.txt"
    wait_for_sync

    rm "$test_dir"/remove*.txt
    wait_for_sync

    assert_file_not_exists "$test_dir/remove1.txt" "Wildcard deleted file 1"
    assert_file_not_exists "$test_dir/remove2.txt" "Wildcard deleted file 2"
    assert_file_not_exists "$test_dir/remove3.txt" "Wildcard deleted file 3"
}

#############################################################################
# TEST 6: Directory Operations
#############################################################################
test_directory_operations() {
    log_test "6" "Directory operations"

    local test_dir="$TEST_DIR/test6"
    mkdir -p "$test_dir"

    # Create single directory
    log_info "Creating directory with mkdir"
    mkdir "$test_dir/dir1"
    wait_for_sync
    assert_dir_exists "$test_dir/dir1" "Directory created"

    # Create nested directories
    log_info "Creating nested directories with mkdir -p"
    mkdir -p "$test_dir/a/b/c/d"
    wait_for_sync
    assert_dir_exists "$test_dir/a/b/c/d" "Nested directories created"

    # Create file in nested directory
    echo "nested" > "$test_dir/a/b/c/d/file.txt"
    wait_for_sync
    assert_file_exists "$test_dir/a/b/c/d/file.txt" "File in nested directory"

    # List directory contents
    log_info "Listing directory contents"
    touch "$test_dir/dir1/file1.txt"
    touch "$test_dir/dir1/file2.txt"
    wait_for_sync

    local file_count
    file_count=$(ls "$test_dir/dir1" | wc -l)
    if [ "$file_count" -eq 2 ]; then
        log_pass "Directory listing shows 2 files"
    else
        log_fail "Expected 2 files, got $file_count"
    fi

    # Remove empty directory
    log_info "Removing empty directory"
    mkdir "$test_dir/empty"
    wait_for_sync
    rmdir "$test_dir/empty"
    wait_for_sync
    if [ ! -d "$test_dir/empty" ]; then
        log_pass "Empty directory removed"
    else
        log_fail "Empty directory still exists"
    fi

    # Remove directory with contents
    log_info "Removing directory tree with rm -rf"
    mkdir -p "$test_dir/tree/sub1/sub2"
    echo "x" > "$test_dir/tree/file.txt"
    echo "x" > "$test_dir/tree/sub1/file.txt"
    wait_for_sync

    rm -rf "$test_dir/tree"
    wait_for_sync
    if [ ! -d "$test_dir/tree" ]; then
        log_pass "Directory tree removed"
    else
        log_fail "Directory tree still exists"
    fi
}

#############################################################################
# TEST 7: Large Files
#############################################################################
test_large_files() {
    log_test "7" "Large file handling"

    local test_dir="$TEST_DIR/test7"
    mkdir -p "$test_dir"

    # Create 1MB file
    log_info "Creating 1MB file"
    dd if=/dev/zero of="$test_dir/1mb.dat" bs=1024 count=1024 2>/dev/null
    wait_for_sync
    assert_file_exists "$test_dir/1mb.dat" "1MB file created"
    assert_file_size_greater "$test_dir/1mb.dat" 1000000 "1MB file has correct size"

    # Create 10MB file
    log_info "Creating 10MB file"
    dd if=/dev/zero of="$test_dir/10mb.dat" bs=1024 count=10240 2>/dev/null
    wait_for_sync
    assert_file_exists "$test_dir/10mb.dat" "10MB file created"
    assert_file_size_greater "$test_dir/10mb.dat" 10000000 "10MB file has correct size"

    # Read from large file
    log_info "Reading from large file"
    if head -c 100 "$test_dir/10mb.dat" > /dev/null 2>&1; then
        log_pass "Can read from large file"
    else
        log_fail "Cannot read from large file"
    fi

    # Copy large file
    log_info "Copying large file"
    cp "$test_dir/1mb.dat" "$test_dir/1mb_copy.dat"
    wait_for_sync
    assert_file_exists "$test_dir/1mb_copy.dat" "Large file copied"
    assert_file_size_greater "$test_dir/1mb_copy.dat" 1000000 "Copied file has correct size"
}

#############################################################################
# TEST 8: Special Characters in Filenames
#############################################################################
test_special_characters() {
    log_test "8" "Special characters in filenames"

    local test_dir="$TEST_DIR/test8"
    mkdir -p "$test_dir"

    # Spaces
    log_info "Testing spaces in filename"
    echo "content" > "$test_dir/file with spaces.txt"
    wait_for_sync
    assert_file_exists "$test_dir/file with spaces.txt" "File with spaces"
    assert_content_equals "$test_dir/file with spaces.txt" "content" "Content of file with spaces"

    # Dashes and underscores
    log_info "Testing dashes and underscores"
    echo "content" > "$test_dir/file-with_dashes.txt"
    wait_for_sync
    assert_file_exists "$test_dir/file-with_dashes.txt" "File with dashes/underscores"

    # Dots
    log_info "Testing multiple dots"
    echo "content" > "$test_dir/file.name.with.dots.txt"
    wait_for_sync
    assert_file_exists "$test_dir/file.name.with.dots.txt" "File with multiple dots"

    # Numbers
    log_info "Testing numbers in filename"
    echo "content" > "$test_dir/file123.txt"
    wait_for_sync
    assert_file_exists "$test_dir/file123.txt" "File with numbers"

    # Parentheses
    log_info "Testing parentheses"
    echo "content" > "$test_dir/file (1).txt"
    wait_for_sync
    assert_file_exists "$test_dir/file (1).txt" "File with parentheses"
}

#############################################################################
# TEST 9: File Content Integrity
#############################################################################
test_content_integrity() {
    log_test "9" "File content integrity"

    local test_dir="$TEST_DIR/test9"
    mkdir -p "$test_dir"

    # Write known content
    log_info "Writing known content"
    local expected="The quick brown fox jumps over the lazy dog. 0123456789 !@#$%^&*()"
    echo "$expected" > "$test_dir/integrity.txt"
    wait_for_sync

    # Read back and verify
    log_info "Verifying content integrity"
    assert_content_equals "$test_dir/integrity.txt" "$expected" "Content integrity preserved"

    # Unicode characters (if supported)
    log_info "Testing Unicode content"
    echo "Hello 世界 🌍" > "$test_dir/unicode.txt"
    wait_for_sync
    if cat "$test_dir/unicode.txt" | grep -q "世界"; then
        log_pass "Unicode content preserved"
    else
        log_fail "Unicode content not preserved"
    fi

    # Binary-like content
    log_info "Testing binary-like content"
    printf '\x00\x01\x02\xFF' > "$test_dir/binary.dat"
    wait_for_sync
    local size
    size=$(stat -c%s "$test_dir/binary.dat" 2>/dev/null || stat -f%z "$test_dir/binary.dat" 2>/dev/null)
    if [ "$size" -eq 4 ]; then
        log_pass "Binary data size correct"
    else
        log_fail "Binary data size incorrect (expected 4, got $size)"
    fi
}

#############################################################################
# TEST 10: Concurrent Operations
#############################################################################
test_concurrent_operations() {
    log_test "10" "Concurrent file operations"

    local test_dir="$TEST_DIR/test10"
    mkdir -p "$test_dir"

    # Create multiple files in quick succession
    log_info "Creating multiple files concurrently"
    for i in {1..10}; do
        (echo "file$i" > "$test_dir/concurrent$i.txt" && sync) &
    done
    wait  # Wait for all background processes to complete

    # Force filesystem sync
    sync

    # Give FUSE/Drive more time to propagate concurrent writes
    log_info "Waiting for concurrent writes to propagate"
    sleep 3

    # Additional check: verify files are visible
    sleep 1

    # Verify all files were created
    local created_count
    created_count=$(ls "$test_dir"/concurrent*.txt 2>/dev/null | wc -l)
    if [ "$created_count" -eq 10 ]; then
        log_pass "All 10 concurrent files created"
    else
        log_fail "Expected 10 files, found $created_count"
    fi

    # Verify content of concurrent files with retry logic
    log_info "Verifying concurrent file contents"
    local content_ok=0
    local failed_files=""

    # Retry up to 3 times with 1 second delay between attempts
    for attempt in {1..3}; do
        content_ok=0
        failed_files=""

        for i in {1..10}; do
            if [ -f "$test_dir/concurrent$i.txt" ]; then
                local content
                content=$(cat "$test_dir/concurrent$i.txt" 2>/dev/null || echo "")
                if [ "$content" = "file$i" ]; then
                    content_ok=$((content_ok + 1))
                else
                    failed_files="$failed_files\n  concurrent$i.txt: expected 'file$i', got '$content'"
                fi
            else
                failed_files="$failed_files\n  concurrent$i.txt: file not found"
            fi
        done

        # If all files are correct, break out of retry loop
        if [ "$content_ok" -eq 10 ]; then
            break
        fi

        # If not the last attempt, wait and retry
        if [ "$attempt" -lt 3 ]; then
            log_info "Retry $attempt: Only $content_ok/10 correct, waiting 1s before retry"
            sleep 1
        fi
    done

    if [ "$content_ok" -eq 10 ]; then
        log_pass "All concurrent file contents correct"
    else
        echo -e "${RED}✗ FAIL: Only $content_ok/10 files have correct content after 3 attempts${NC}"
        echo -e "${RED}Failed files:$failed_files${NC}"
        ((TEST_FAILED++))
    fi
}

#############################################################################
# TEST 11: Copy Operations
#############################################################################
test_copy_operations() {
    log_test "11" "File copy operations"

    local test_dir="$TEST_DIR/test11"
    mkdir -p "$test_dir/source"
    mkdir -p "$test_dir/dest"

    # Create source file
    echo "original content" > "$test_dir/source/original.txt"
    wait_for_sync

    # Simple copy
    log_info "Simple file copy with cp"
    cp "$test_dir/source/original.txt" "$test_dir/dest/copy.txt"
    wait_for_sync

    assert_file_exists "$test_dir/source/original.txt" "Original file still exists"
    assert_file_exists "$test_dir/dest/copy.txt" "Copy created"
    assert_content_equals "$test_dir/dest/copy.txt" "original content" "Copy content matches"

    # Copy and modify
    log_info "Modifying copy doesn't affect original"
    echo "modified content" > "$test_dir/dest/copy.txt"
    wait_for_sync

    assert_content_equals "$test_dir/source/original.txt" "original content" "Original unchanged"
    assert_content_equals "$test_dir/dest/copy.txt" "modified content" "Copy modified"

    # Copy directory
    log_info "Copying directory recursively"
    mkdir -p "$test_dir/source/subdir"
    echo "sub content" > "$test_dir/source/subdir/file.txt"
    wait_for_sync

    cp -r "$test_dir/source" "$test_dir/dest/source_copy"
    wait_for_sync

    assert_dir_exists "$test_dir/dest/source_copy" "Directory copied"
    assert_file_exists "$test_dir/dest/source_copy/subdir/file.txt" "Subdirectory file copied"
    assert_content_equals "$test_dir/dest/source_copy/subdir/file.txt" "sub content" "Subdirectory file content"
}

#############################################################################
# TEST 12: Duplicate File Handling - Same Directory
#############################################################################
test_duplicate_same_directory() {
    log_test "12" "Duplicate file handling in same directory"

    local test_dir="$TEST_DIR/test12"
    mkdir -p "$test_dir"

    log_info "Creating single file 'photo.jpg'"
    echo "content1" > "$test_dir/photo.jpg"
    wait_for_sync

    # Verify single file exists without suffix
    assert_file_exists "$test_dir/photo.jpg" "Single file exists without suffix"
    assert_file_not_exists "$test_dir/photo.jpg.1" "No .1 suffix for unique file"
    assert_content_equals "$test_dir/photo.jpg" "content1" "File content correct"

    log_info "Note: True Drive duplicates in same directory would appear with .1, .2 suffixes"
    log_info "This test validates that unique files don't get suffixes incorrectly"
}

#############################################################################
# TEST 13: Same Filename in Different Directories
#############################################################################
test_same_name_different_dirs() {
    log_test "13" "Same filename in different directories (no suffixes)"

    local test_dir="$TEST_DIR/test13"
    mkdir -p "$test_dir/dir1"
    mkdir -p "$test_dir/dir2"

    log_info "Creating 'photo.jpg' in dir1"
    echo "content_dir1" > "$test_dir/dir1/photo.jpg"
    wait_for_sync

    log_info "Creating 'photo.jpg' in dir2"
    echo "content_dir2" > "$test_dir/dir2/photo.jpg"
    wait_for_sync

    # Files in different directories should NOT have suffixes
    assert_file_exists "$test_dir/dir1/photo.jpg" "File in dir1 exists without suffix"
    assert_file_exists "$test_dir/dir2/photo.jpg" "File in dir2 exists without suffix"

    # Verify NO suffixed versions exist
    assert_file_not_exists "$test_dir/dir1/photo.jpg.1" "No .1 suffix in dir1"
    assert_file_not_exists "$test_dir/dir2/photo.jpg.1" "No .1 suffix in dir2"

    # Verify contents are independent
    assert_content_equals "$test_dir/dir1/photo.jpg" "content_dir1" "Dir1 content correct"
    assert_content_equals "$test_dir/dir2/photo.jpg" "content_dir2" "Dir2 content correct"
}

#############################################################################
# TEST 14: Nested Directories with Same Filenames
#############################################################################
test_nested_same_names() {
    log_test "14" "Same filename in nested directories"

    local test_dir="$TEST_DIR/test14"
    mkdir -p "$test_dir/a/b/c"
    mkdir -p "$test_dir/a/b/d"
    mkdir -p "$test_dir/x/y/z"

    # Create files with same name in different nested directories
    log_info "Creating 'data.txt' in multiple nested paths"
    echo "content_abc" > "$test_dir/a/b/c/data.txt"
    echo "content_abd" > "$test_dir/a/b/d/data.txt"
    echo "content_xyz" > "$test_dir/x/y/z/data.txt"
    wait_for_sync

    # All should exist without suffixes (different parent directories)
    assert_file_exists "$test_dir/a/b/c/data.txt" "data.txt in a/b/c"
    assert_file_exists "$test_dir/a/b/d/data.txt" "data.txt in a/b/d"
    assert_file_exists "$test_dir/x/y/z/data.txt" "data.txt in x/y/z"

    # Verify no suffixes
    assert_file_not_exists "$test_dir/a/b/c/data.txt.1" "No suffix in a/b/c"
    assert_file_not_exists "$test_dir/a/b/d/data.txt.1" "No suffix in a/b/d"
    assert_file_not_exists "$test_dir/x/y/z/data.txt.1" "No suffix in x/y/z"

    # Verify independent contents
    assert_content_equals "$test_dir/a/b/c/data.txt" "content_abc" "Content in a/b/c"
    assert_content_equals "$test_dir/a/b/d/data.txt" "content_abd" "Content in a/b/d"
    assert_content_equals "$test_dir/x/y/z/data.txt" "content_xyz" "Content in x/y/z"
}

#############################################################################
# Main Execution
#############################################################################
main() {
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}  GCSF Comprehensive Filesystem Operations Test${NC}"
    echo -e "${BLUE}================================================${NC}"
    echo ""
    echo "Mount point: $MOUNT_POINT"
    echo "Test directory: $TEST_DIR"
    echo ""

    # Create main test directory
    mkdir -p "$TEST_DIR"

    # Run all tests
    test_file_creation || true
    test_file_writing || true
    test_file_reading || true
    test_file_moving || true
    test_file_deletion || true
    test_directory_operations || true
    test_large_files || true
    test_special_characters || true
    test_content_integrity || true
    test_concurrent_operations || true
    test_copy_operations || true
    test_duplicate_same_directory || true
    test_same_name_different_dirs || true
    test_nested_same_names || true

    # Print summary
    echo ""
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}  Test Summary${NC}"
    echo -e "${BLUE}================================================${NC}"
    echo "Tests run:    $TESTS_RUN"
    echo -e "Tests passed: ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Tests failed: ${RED}$TESTS_FAILED${NC}"

    if [ $TESTS_PASSED -gt 0 ]; then
        local pass_rate=$((TESTS_PASSED * 100 / (TESTS_PASSED + TESTS_FAILED)))
        echo "Pass rate:    ${pass_rate}%"
    fi
    echo ""

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "${GREEN}✓✓✓ All tests passed! GCSF is working correctly ✓✓✓${NC}"
        exit 0
    else
        echo -e "${YELLOW}⚠ Some tests failed - see details above${NC}"
        exit 1
    fi
}

# Run main function
main
