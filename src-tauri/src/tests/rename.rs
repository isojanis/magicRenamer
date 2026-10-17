use super::*;
use crate::test_support::*;
use std::collections::HashSet;

#[tokio::test]
async fn rename_collision_manifest_hash_and_undo_are_safe() {
    let folder = unique_temp_dir("rename-safety");
    let source = folder.join("original.txt");
    let destination = folder.join("report.txt");
    let collision_one = folder.join("report_1.txt");
    fs::write(&source, "document bytes").unwrap();
    fs::write(&destination, "keep existing destination").unwrap();
    fs::write(&collision_one, "keep collision one").unwrap();
    let expected_hash = compute_file_sha256(&source).unwrap();

    let renamed = rename_file(
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap();
    let renamed_path = Path::new(&renamed);
    assert_eq!(renamed_path.file_name().unwrap(), "report_2.txt");
    assert_eq!(fs::read(&destination).unwrap(), b"keep existing destination");
    assert_eq!(fs::read(&collision_one).unwrap(), b"keep collision one");
    assert_eq!(fs::read(renamed_path).unwrap(), b"document bytes");

    let manifest_path = fs::read_dir(&folder).unwrap().filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with("rename_history_") && path.extension().is_some_and(|ext| ext == "json"))
        .expect("rename manifest should be written");
    let manifest: RenameHistoryManifest = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.mappings.len(), 1);
    assert_eq!(manifest.mappings[0].current_name, "report_2.txt");
    assert_eq!(manifest.mappings[0].original_name, "original.txt");
    assert_eq!(manifest.mappings[0].sha256, expected_hash);

    remove_xattr_original_name(renamed_path);
    fs::write(&source, "preserve this original").unwrap();
    let error = undo_rename_file(renamed.clone()).await.unwrap_err();
    assert!(error.contains("already exists"));
    assert_eq!(fs::read(&source).unwrap(), b"preserve this original");
    assert_eq!(fs::read(renamed_path).unwrap(), b"document bytes");

    fs::remove_file(&source).unwrap();
    let undo = undo_rename_file(renamed_path.to_string_lossy().into_owned()).await.unwrap();
    assert_eq!(undo.source, "manifest");
    assert_eq!(undo.original_name, "original.txt");
    assert_eq!(fs::read(&source).unwrap(), b"document bytes");
    remove_temp_dir(&folder);
}

#[tokio::test]
async fn multiple_renames_append_mappings_to_the_session_manifest() {
    let folder = unique_temp_dir("manifest-append");
    for index in 1..=2 {
        let source = folder.join(format!("draft-{}.txt", index));
        let destination = folder.join(format!("report-{}.txt", index));
        fs::write(&source, format!("document {}", index)).unwrap();
        rename_file(
            source.to_string_lossy().into_owned(),
            destination.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
    }
    let manifest_path = fs::read_dir(&folder)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with("rename_history_") && path.extension().is_some_and(|ext| ext == "json"))
        .expect("manifest should exist");
    let manifest: RenameHistoryManifest = serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.mappings.len(), 2);
    assert!(manifest.mappings.iter().any(|mapping| mapping.original_name == "draft-1.txt"));
    assert!(manifest.mappings.iter().any(|mapping| mapping.original_name == "draft-2.txt"));
}

#[tokio::test]
async fn undo_without_history_and_missing_files_returns_errors() {
    let folder = unique_temp_dir("undo-errors");
    let current = folder.join("renamed.txt");
    fs::write(&current, "content").unwrap();
    let error = undo_rename_file(current.to_string_lossy().into_owned()).await.unwrap_err();
    assert!(error.contains("No original filename record"));
    let missing = folder.join("missing.txt");
    assert!(undo_rename_file(missing.to_string_lossy().into_owned()).await.unwrap_err().contains("does not exist"));
}

#[tokio::test]
async fn rename_refuses_to_move_files_outside_their_folder() {
    let folder = unique_temp_dir("rename-folder");
    let outside = unique_temp_dir("rename-outside");
    let source = folder.join("draft.txt");
    let destination = outside.join("renamed.txt");
    fs::write(&source, "keep me here").unwrap();
    let error = rename_file(
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("current folder"));
    assert!(source.exists());
    assert!(!destination.exists());
    remove_temp_dir(&folder);
    remove_temp_dir(&outside);
}

#[tokio::test]
async fn rename_rolls_back_when_manifest_cannot_be_written() {
    let folder = unique_temp_dir("rename-manifest-failure");
    let source = folder.join("draft.txt");
    let destination = folder.join("renamed.txt");
    fs::write(&source, "keep original bytes").unwrap();
    let manifest_dir = folder.join(format!("rename_history_{}.json", Utc::now().format("%Y%m%d_%H%M")));
    fs::create_dir(&manifest_dir).unwrap();

    let error = rename_file(
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("rolled back"));
    assert_eq!(fs::read(&source).unwrap(), b"keep original bytes");
    assert!(!destination.exists());
    remove_temp_dir(&folder);
}

#[test]
fn manifest_write_failure_is_reported() {
    let folder = unique_temp_dir("manifest-error");
    let not_a_directory = folder.join("file");
    fs::write(&not_a_directory, "file").unwrap();
    let error = update_or_create_manifest(
        &not_a_directory,
        RenameMapping {
            current_name: "renamed.txt".into(),
            original_name: "original.txt".into(),
            sha256: "hash".into(),
            renamed_at: Utc::now().to_rfc3339(),
        },
    )
    .unwrap_err();
    assert!(error.contains("manifest") || !error.is_empty());
    remove_temp_dir(&folder);
}

#[tokio::test]
async fn manifest_undo_rejects_changed_content() {
    let folder = unique_temp_dir("undo-hash-mismatch");
    let source = folder.join("original.txt");
    let destination = folder.join("renamed.txt");
    fs::write(&source, "original content").unwrap();
    let renamed = rename_file(
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap();
    remove_xattr_original_name(Path::new(&renamed));
    fs::write(&renamed, "changed content").unwrap();

    let error = undo_rename_file(renamed.clone()).await.unwrap_err();
    assert!(error.contains("SHA-256"), "{}", error);
    assert!(!source.exists());
    assert_eq!(fs::read(&renamed).unwrap(), b"changed content");

    fs::write(&renamed, "original content").unwrap();
    let restored = undo_rename_file(renamed).await.unwrap();
    assert_eq!(restored.source, "manifest");
    assert_eq!(fs::read(&source).unwrap(), b"original content");
}

#[tokio::test]
async fn malformed_manifest_is_preserved_and_rename_is_rolled_back() {
    let folder = unique_temp_dir("corrupt-manifest");
    let source = folder.join("draft.txt");
    let destination = folder.join("report.txt");
    let manifest = folder.join(format!(
        "rename_history_{}.json",
        Utc::now().format("%Y%m%d_%H%M")
    ));
    fs::write(&source, "preserve document").unwrap();
    fs::write(&manifest, "{bad json").unwrap();

    let error = rename_file(
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("rolled back"), "{}", error);
    assert_eq!(fs::read(&manifest).unwrap(), b"{bad json");
    assert_eq!(fs::read(&source).unwrap(), b"preserve document");
    assert!(!destination.exists());
}

#[tokio::test]
async fn nonregular_sources_are_rejected_for_rename_and_undo() {
    let folder = unique_temp_dir("nonregular-source");
    let directory = folder.join("directory.txt");
    fs::create_dir(&directory).unwrap();
    let destination = folder.join("renamed.txt");
    for operation in [
        rename_file(
            directory.to_string_lossy().into_owned(),
            destination.to_string_lossy().into_owned(),
        )
        .await
        .unwrap_err(),
        undo_rename_file(directory.to_string_lossy().into_owned())
            .await
            .unwrap_err(),
    ] {
        assert!(operation.contains("regular files"), "{}", operation);
    }
    assert!(directory.is_dir());
    assert!(!destination.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_sources_are_rejected_and_dangling_destination_is_preserved() {
    use std::os::unix::fs::symlink;

    let folder = unique_temp_dir("symlink-safety");
    let real = folder.join("real.txt");
    let link = folder.join("link.txt");
    let destination = folder.join("report.txt");
    fs::write(&real, "real content").unwrap();
    symlink(&real, &link).unwrap();
    let error = rename_file(
        link.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("regular files"), "{}", error);
    let error = undo_rename_file(link.to_string_lossy().into_owned())
        .await
        .unwrap_err();
    assert!(error.contains("regular files"), "{}", error);

    symlink(folder.join("missing.txt"), &destination).unwrap();
    let renamed = rename_file(
        real.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap();
    assert_eq!(Path::new(&renamed).file_name().unwrap(), "report_1.txt");
    assert!(fs::symlink_metadata(&destination).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(renamed).unwrap(), b"real content");
}

#[cfg(unix)]
#[tokio::test]
async fn undo_preserves_a_dangling_original_name_symlink() {
    use std::os::unix::fs::symlink;

    let folder = unique_temp_dir("undo-dangling-symlink");
    let original = folder.join("original.txt");
    let destination = folder.join("renamed.txt");
    fs::write(&original, "document").unwrap();
    let renamed = rename_file(
        original.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    )
    .await
    .unwrap();
    remove_xattr_original_name(Path::new(&renamed));
    symlink(folder.join("absent.txt"), &original).unwrap();

    let error = undo_rename_file(renamed.clone()).await.unwrap_err();
    assert!(error.contains("already exists"), "{}", error);
    assert!(fs::symlink_metadata(&original).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&renamed).unwrap(), b"document");
}

#[test]
fn no_replace_move_preserves_an_occupied_destination() {
    let folder = unique_temp_dir("no-replace");
    let source = folder.join("source.txt");
    let destination = folder.join("destination.txt");
    fs::write(&source, "source bytes").unwrap();
    fs::write(&destination, "destination bytes").unwrap();
    let error = move_file_no_replace(&source, &destination).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&source).unwrap(), b"source bytes");
    assert_eq!(fs::read(&destination).unwrap(), b"destination bytes");
}

#[test]
fn simultaneous_renames_keep_all_files_and_manifest_entries() {
    let folder = unique_temp_dir("concurrent-renames");
    let mut workers = Vec::new();
    for index in 0..8 {
        let source = folder.join(format!("draft-{}.txt", index));
        let destination = folder.join("report.txt");
        fs::write(&source, format!("document {}", index)).unwrap();
        workers.push(std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            runtime.block_on(rename_file(
                source.to_string_lossy().into_owned(),
                destination.to_string_lossy().into_owned(),
            ))
        }));
    }
    let mut renamed = HashSet::new();
    for worker in workers {
        let path = worker.join().unwrap().unwrap();
        assert!(renamed.insert(path.clone()), "duplicate destination: {}", path);
    }
    assert_eq!(renamed.len(), 8);
    let mut contents = HashSet::new();
    for path in renamed {
        contents.insert(fs::read_to_string(path).unwrap());
    }
    assert_eq!(contents.len(), 8);
    for index in 0..8 {
        assert!(contents.contains(&format!("document {}", index)));
    }
    let mut recorded_originals = HashSet::new();
    for entry in fs::read_dir(&folder).unwrap().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("rename_history_") && name.ends_with(".json") {
            let history: RenameHistoryManifest =
                serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
            for mapping in history.mappings {
                assert!(recorded_originals.insert(mapping.original_name));
            }
        }
    }
    assert_eq!(recorded_originals.len(), 8);
}

#[tokio::test]
async fn rename_and_undo_with_manifest_and_xattr() {
    let temp_dir = unique_temp_dir("undo-test");

    let test_file = temp_dir.join("original_draft_v1.txt");
    fs::write(&test_file, "Document content to be hashed and tracked").unwrap();

    let new_file_path = temp_dir.join("2026-10-17_Report_Final.txt");

    // 1. Rename file
    let renamed_path_str = rename_file(
        test_file.to_string_lossy().to_string(),
        new_file_path.to_string_lossy().to_string(),
    )
    .await
    .expect("rename_file should succeed");

    assert!(Path::new(&renamed_path_str).exists());
    assert!(!test_file.exists());

    // 2. Verify manifest was created in temp_dir
    let mut manifest_found = false;
    if let Ok(entries) = fs::read_dir(&temp_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("rename_history_") && name.ends_with(".json") {
                manifest_found = true;
                let content = fs::read_to_string(entry.path()).unwrap();
                let manifest: RenameHistoryManifest = serde_json::from_str(&content).unwrap();
                assert_eq!(manifest.mappings.len(), 1);
                assert_eq!(manifest.mappings[0].original_name, "original_draft_v1.txt");
                assert_eq!(manifest.mappings[0].current_name, "2026-10-17_Report_Final.txt");
                assert!(!manifest.mappings[0].sha256.is_empty());
            }
        }
    }
    assert!(manifest_found, "rename_history_YYYYMMDD_HHMM.json should be present");

    // 3. Test undo_rename_file
    let undo_result = undo_rename_file(renamed_path_str)
        .await
        .expect("undo_rename_file should revert to original name");

    assert_eq!(undo_result.original_name, "original_draft_v1.txt");
    assert!(test_file.exists());
    assert!(!new_file_path.exists());

    let _ = fs::remove_dir_all(&temp_dir);
}
