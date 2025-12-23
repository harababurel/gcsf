#[cfg(test)]
mod rename_identical_files_tests {
    use crate::gcsf::{File, FileId, FileManager};

    // Helper to create a test file
    fn create_test_file(name: &str, inode: u64, drive_id: Option<String>) -> File {
        use fuser::{FileAttr, FileType};
        use std::time::SystemTime;

        let drive_file = drive3::api::File {
            id: drive_id,
            name: Some(name.to_string()),
            ..Default::default()
        };

        File {
            name: name.to_string(),
            attr: FileAttr {
                ino: inode,
                size: 1024,
                blocks: 2,
                blksize: 512,
                atime: SystemTime::UNIX_EPOCH,
                mtime: SystemTime::UNIX_EPOCH,
                ctime: SystemTime::UNIX_EPOCH,
                crtime: SystemTime::UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o644,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive_file),
        }
    }

    // Helper to create a test directory
    fn create_test_directory(name: &str, inode: u64, drive_id: Option<String>) -> File {
        use fuser::{FileAttr, FileType};
        use std::time::SystemTime;

        let drive_file = drive3::api::File {
            id: drive_id,
            name: Some(name.to_string()),
            ..Default::default()
        };

        File {
            name: name.to_string(),
            attr: FileAttr {
                ino: inode,
                size: 512,
                blocks: 1,
                blksize: 512,
                atime: SystemTime::UNIX_EPOCH,
                mtime: SystemTime::UNIX_EPOCH,
                ctime: SystemTime::UNIX_EPOCH,
                crtime: SystemTime::UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o755,
                nlink: 2,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                flags: 0,
            },
            identical_name_id: None,
            drive_file: Some(drive_file),
        }
    }

    #[test]
    fn test_file_name_method_no_suffix() {
        let file = create_test_file("test.txt", 100, Some("drive_id_1".to_string()));

        // No suffix when identical_name_id is None
        assert_eq!(file.name(), "test.txt");
    }

    #[test]
    fn test_file_name_method_with_suffix() {
        let mut file = create_test_file("test.txt", 100, Some("drive_id_1".to_string()));

        // With suffix
        file.identical_name_id = Some(1);
        assert_eq!(file.name(), "test.txt.1");

        file.identical_name_id = Some(42);
        assert_eq!(file.name(), "test.txt.42");
    }

    #[test]
    fn test_file_name_preserves_extension() {
        let mut file = create_test_file("document.pdf", 101, Some("drive_id_2".to_string()));

        file.identical_name_id = Some(2);
        assert_eq!(file.name(), "document.pdf.2");
    }

    #[test]
    fn test_file_name_no_extension() {
        let mut file = create_test_file("README", 102, Some("drive_id_3".to_string()));

        file.identical_name_id = Some(1);
        assert_eq!(file.name(), "README.1");
    }

    /// Test: Duplicates in same directory get suffixes
    /// First file (by Drive ID) gets no suffix, second gets .1
    #[test]
    fn test_duplicates_same_directory_get_suffixes() {
        let mut fm = FileManager::new_for_testing(true);

        // Add two files with same name to same parent (ROOT)
        let file1 = create_test_file("photo.jpg", 101, Some("drive_id_a".to_string()));
        let file2 = create_test_file("photo.jpg", 102, Some("drive_id_b".to_string()));

        fm.add_test_file(file1, 1).unwrap(); // ROOT_INODE = 1
        fm.add_test_file(file2, 1).unwrap();

        fm.recalculate_duplicate_suffixes_for_parent(1);

        // Verify: first file (by Drive ID) has no suffix, second has .1
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().identical_name_id,
            None,
            "First file should have no suffix"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(102)).unwrap().identical_name_id,
            Some(1),
            "Second file should have suffix .1"
        );

        // Verify name() method returns correct names
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().name(),
            "photo.jpg"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(102)).unwrap().name(),
            "photo.jpg.1"
        );

        // Prevent drop to avoid undefined behavior with uninitialized DriveFacade
        std::mem::forget(fm);
    }

    /// Test: Same name in different directories - NO suffixes (THE BUG REGRESSION TEST)
    /// This is the critical test that verifies the bug was fixed.
    #[test]
    fn test_same_name_different_directories_no_suffixes() {
        let mut fm = FileManager::new_for_testing(true);

        // Create two directories under root
        let dir1 = create_test_directory("dir1", 100, Some("dir_a".to_string()));
        let dir2 = create_test_directory("dir2", 200, Some("dir_b".to_string()));
        fm.add_test_file(dir1, 1).unwrap(); // ROOT_INODE = 1
        fm.add_test_file(dir2, 1).unwrap();

        // Add same-named file to each directory
        let file1 = create_test_file("photo.jpg", 101, Some("drive_id_a".to_string()));
        let file2 = create_test_file("photo.jpg", 201, Some("drive_id_b".to_string()));

        fm.add_test_file(file1, 100).unwrap();
        fm.add_test_file(file2, 200).unwrap();

        fm.recalculate_all_duplicate_suffixes();

        // Verify: BOTH files have no suffix (different directories)
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().identical_name_id,
            None,
            "File in dir1 should have NO suffix (different parent)"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(201)).unwrap().identical_name_id,
            None,
            "File in dir2 should have NO suffix (different parent)"
        );

        // Verify name() method returns correct names (no suffix)
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().name(),
            "photo.jpg"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(201)).unwrap().name(),
            "photo.jpg"
        );

        // Prevent drop to avoid undefined behavior with uninitialized DriveFacade
        std::mem::forget(fm);
    }

    /// Test: Three duplicates get .1, .2 numbering
    #[test]
    fn test_three_duplicates_correct_numbering() {
        let mut fm = FileManager::new_for_testing(true);

        // Add three files with same name to same parent
        let file1 = create_test_file("doc.txt", 101, Some("drive_id_1".to_string()));
        let file2 = create_test_file("doc.txt", 102, Some("drive_id_2".to_string()));
        let file3 = create_test_file("doc.txt", 103, Some("drive_id_3".to_string()));

        fm.add_test_file(file1, 1).unwrap();
        fm.add_test_file(file2, 1).unwrap();
        fm.add_test_file(file3, 1).unwrap();

        fm.recalculate_duplicate_suffixes_for_parent(1);

        // Verify: first gets no suffix, rest get .1, .2
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().identical_name_id,
            None
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(102)).unwrap().identical_name_id,
            Some(1)
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(103)).unwrap().identical_name_id,
            Some(2)
        );

        // Verify names
        assert_eq!(fm.get_file(&FileId::Inode(101)).unwrap().name(), "doc.txt");
        assert_eq!(
            fm.get_file(&FileId::Inode(102)).unwrap().name(),
            "doc.txt.1"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(103)).unwrap().name(),
            "doc.txt.2"
        );

        // Prevent drop to avoid undefined behavior with uninitialized DriveFacade
        std::mem::forget(fm);
    }

    /// Test: Drive ID ordering is deterministic
    /// Files are sorted by Drive ID - first alphabetically gets no suffix
    #[test]
    fn test_drive_id_ordering_stable() {
        let mut fm = FileManager::new_for_testing(true);

        // Add files with Drive IDs in non-alphabetical order
        let file_z = create_test_file("test.txt", 101, Some("z_last".to_string()));
        let file_a = create_test_file("test.txt", 102, Some("a_first".to_string()));
        let file_m = create_test_file("test.txt", 103, Some("m_middle".to_string()));

        fm.add_test_file(file_z, 1).unwrap();
        fm.add_test_file(file_a, 1).unwrap();
        fm.add_test_file(file_m, 1).unwrap();

        fm.recalculate_duplicate_suffixes_for_parent(1);

        // Verify: "a_first" gets no suffix, "m_middle" gets .1, "z_last" gets .2
        assert_eq!(
            fm.get_file(&FileId::Inode(102)).unwrap().identical_name_id,
            None,
            "File with Drive ID 'a_first' should have no suffix"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(103)).unwrap().identical_name_id,
            Some(1),
            "File with Drive ID 'm_middle' should have suffix .1"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().identical_name_id,
            Some(2),
            "File with Drive ID 'z_last' should have suffix .2"
        );

        // Prevent drop to avoid undefined behavior with uninitialized DriveFacade
        std::mem::forget(fm);
    }

    /// Test: Suffix removed when file is the only one with that name
    #[test]
    fn test_suffix_removed_when_only_one_remains() {
        let mut fm = FileManager::new_for_testing(true);

        // Add one file
        let file = create_test_file("unique.txt", 101, Some("drive_id_a".to_string()));
        fm.add_test_file(file, 1).unwrap();

        fm.recalculate_duplicate_suffixes_for_parent(1);

        // Verify: single file has no suffix
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().identical_name_id,
            None,
            "Single file should have no suffix"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().name(),
            "unique.txt"
        );

        // Prevent drop to avoid undefined behavior with uninitialized DriveFacade
        std::mem::forget(fm);
    }

    /// Test: Files without Drive ID are handled correctly
    /// Files with None Drive ID should sort before those with Some(...)
    #[test]
    fn test_files_without_drive_id() {
        let mut fm = FileManager::new_for_testing(true);

        // Add files: one without Drive ID, one with Drive ID
        let file_no_id = create_test_file("file.txt", 101, None);
        let file_with_id = create_test_file("file.txt", 102, Some("drive_id_z".to_string()));

        fm.add_test_file(file_no_id, 1).unwrap();
        fm.add_test_file(file_with_id, 1).unwrap();

        fm.recalculate_duplicate_suffixes_for_parent(1);

        // Verify: None sorts first (no suffix), file with ID gets suffix
        assert_eq!(
            fm.get_file(&FileId::Inode(101)).unwrap().identical_name_id,
            None,
            "File without Drive ID should have no suffix"
        );
        assert_eq!(
            fm.get_file(&FileId::Inode(102)).unwrap().identical_name_id,
            Some(1),
            "File with Drive ID should have suffix .1"
        );

        // Prevent drop to avoid undefined behavior with uninitialized DriveFacade
        std::mem::forget(fm);
    }
}
