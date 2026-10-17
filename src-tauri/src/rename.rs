use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

fn validate_rename_destination(source: &Path, destination: &Path) -> Result<(), String> {
    let source_parent = source.parent().unwrap_or_else(|| Path::new("."));
    let destination_parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let source_parent = fs::canonicalize(source_parent)
        .map_err(|error| format!("Failed to resolve source folder: {}", error))?;
    let destination_parent = fs::canonicalize(destination_parent)
        .map_err(|error| format!("Failed to resolve destination folder: {}", error))?;
    if source_parent != destination_parent {
        return Err("A file can only be renamed within its current folder".into());
    }
    let name = destination.file_name().and_then(|value| value.to_str())
        .ok_or_else(|| "Destination must be a filename".to_string())?;
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("Destination must be a filename".into());
    }
    Ok(())
}

fn validate_original_name(name: &str) -> Result<(), String> {
    let path = Path::new(name);
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') || path.file_name().and_then(|value| value.to_str()) != Some(name) {
        return Err("Rename history contains an invalid original filename".into());
    }
    Ok(())
}

#[cfg(unix)]
const XATTR_ORIGINAL_NAME: &str = "user.magicrenamer.original_name";
#[cfg(windows)]
const ADS_ORIGINAL_NAME: &str = ":magicrenamer.original_name";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RenameMapping {
    pub current_name: String,
    pub original_name: String,
    pub sha256: String,
    pub renamed_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RenameHistoryManifest {
    pub version: u32,
    pub session_timestamp: String,
    pub mappings: Vec<RenameMapping>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UndoResult {
    pub restored_path: String,
    pub original_name: String,
    pub source: String, // "xattr" or "ads" or "manifest"
}

fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("Failed to open file for SHA-256: {}", e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("Failed to hash file: {}", e))?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn set_xattr_original_name(path: &Path, original_name: &str) {
    let _ = xattr::set(path, XATTR_ORIGINAL_NAME, original_name.as_bytes());
}

#[cfg(unix)]
fn get_xattr_original_name(path: &Path) -> Option<String> {
    xattr::get(path, XATTR_ORIGINAL_NAME)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(unix)]
fn remove_xattr_original_name(path: &Path) {
    let _ = xattr::remove(path, XATTR_ORIGINAL_NAME);
}

#[cfg(windows)]
fn original_name_stream(path: &Path) -> std::path::PathBuf {
    let mut stream = path.as_os_str().to_os_string();
    stream.push(ADS_ORIGINAL_NAME);
    stream.into()
}

#[cfg(windows)]
fn set_xattr_original_name(path: &Path, original_name: &str) {
    let _ = fs::write(original_name_stream(path), original_name.as_bytes());
}

#[cfg(windows)]
fn get_xattr_original_name(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = fs::File::open(original_name_stream(path)).ok()?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(windows)]
fn remove_xattr_original_name(path: &Path) {
    let _ = fs::remove_file(original_name_stream(path));
}

fn require_regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("File does not exist: {}", path.display())
        } else {
            format!("Cannot access file {}: {}", path.display(), error)
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(format!("Only regular files can be renamed or restored: {}", path.display()));
    }
    Ok(())
}

fn lock_directory(dir: &Path) -> Result<fs::File, String> {
    let lock_path = dir.join(".magicrenamer.lock");
    match fs::symlink_metadata(&lock_path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(format!("Rename lock is not a regular file: {}", lock_path.display()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect rename lock: {}", error)),
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
        .map_err(|error| format!("Failed to open rename lock: {}", error))?;
    if !fs::symlink_metadata(&lock_path)
        .map_err(|error| format!("Failed to inspect rename lock: {}", error))?
        .file_type()
        .is_file()
    {
        return Err(format!("Rename lock is not a regular file: {}", lock_path.display()));
    }
    lock.lock()
        .map_err(|error| format!("Failed to lock rename folder: {}", error))?;
    Ok(lock)
}

static MANIFEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_manifest_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "Manifest has no parent folder".to_string())?;
    let mut temp = None;
    for _ in 0..16 {
        let nonce = MANIFEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".rename_history_{}-{}-{}.tmp",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            nonce
        ));
        match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => {
                temp = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Failed to create temporary manifest: {}", error)),
        }
    }
    let (temp_path, mut file) = temp.ok_or_else(|| "Failed to create a unique temporary manifest".to_string())?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| format!("Failed to write temporary manifest: {}", error))?;
        file.sync_all()
            .map_err(|error| format!("Failed to sync temporary manifest: {}", error))?;
        drop(file);
        fs::rename(&temp_path, path)
            .map_err(|error| format!("Failed to replace manifest: {}", error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn update_or_create_manifest_locked(dir: &Path, mapping: RenameMapping) -> Result<(), String> {
    let now = Utc::now();
    let target = dir.join(format!("rename_history_{}.json", now.format("%Y%m%d_%H%M")));
    let mut manifest = match fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(format!("Rename manifest is not a regular file: {}", target.display()));
            }
            let content = fs::read_to_string(&target)
                .map_err(|error| format!("Failed to read rename manifest: {}", error))?;
            serde_json::from_str::<RenameHistoryManifest>(&content)
                .map_err(|error| format!("Failed to parse rename manifest: {}", error))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => RenameHistoryManifest {
            version: 1,
            session_timestamp: now.to_rfc3339(),
            mappings: Vec::new(),
        },
        Err(error) => return Err(format!("Failed to inspect rename manifest: {}", error)),
    };
    if manifest.version != 1 {
        return Err(format!("Unsupported rename manifest version: {}", manifest.version));
    }
    if let Some(existing) = manifest.mappings.iter_mut().find(|m| m.current_name == mapping.current_name) {
        *existing = mapping;
    } else {
        manifest.mappings.push(mapping);
    }
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Failed to serialize rename manifest: {}", error))?;
    write_manifest_atomically(&target, &bytes)
}

#[cfg(test)]
fn update_or_create_manifest(dir: &Path, mapping: RenameMapping) -> Result<(), String> {
    let _lock = lock_directory(dir)?;
    update_or_create_manifest_locked(dir, mapping)
}

// A hard link reserves the destination name atomically and fails if it is occupied.
// The source and destination are always in the same folder and are regular files.
fn move_file_no_replace(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::hard_link(source, destination)?;
    fs::remove_file(source)
}

/// Renames a regular file within its folder, preserving existing destinations and recording history.
#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) async fn rename_file(old_path: String, new_path: String) -> Result<String, String> {
    let source = Path::new(&old_path);
    require_regular_file(source)?;
    let destination = Path::new(&new_path);
    validate_rename_destination(source, destination)?;
    if source.file_name() == destination.file_name() {
        return Err("The new filename must differ from the current filename".to_string());
    }
    let parent = source.parent().ok_or_else(|| "File has no parent directory".to_string())?;
    let _lock = lock_directory(parent)?;
    require_regular_file(source)?;
    let original_name = source.file_name().and_then(|name| name.to_str())
        .ok_or_else(|| "Source must have a filename".to_string())?.to_string();
    let sha256 = compute_file_sha256(source)?;

    let stem = destination.file_stem().and_then(|part| part.to_str()).unwrap_or("file");
    let extension = destination.extension().and_then(|part| part.to_str())
        .map(|part| format!(".{}", part)).unwrap_or_default();
    let mut counter = 0u32;
    let final_destination = loop {
        let candidate = if counter == 0 {
            destination.to_path_buf()
        } else {
            parent.join(format!("{}_{}{}", stem, counter, extension))
        };
        match move_file_no_replace(source, &candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                counter = counter.checked_add(1)
                    .ok_or_else(|| "No available destination filename".to_string())?;
            }
            Err(error) => {
                return Err(format!(
                    "Failed to rename file: {}. If the destination was created, both paths may remain: {} and {}",
                    error, source.display(), candidate.display()
                ));
            }
        }
    };

    set_xattr_original_name(&final_destination, &original_name);
    let current_name = final_destination.file_name().and_then(|name| name.to_str())
        .ok_or_else(|| "Destination must have a filename".to_string())?.to_string();
    let mapping = RenameMapping {
        current_name,
        original_name,
        sha256,
        renamed_at: Utc::now().to_rfc3339(),
    };
    if let Err(manifest_error) = update_or_create_manifest_locked(parent, mapping) {
        if let Err(rollback_error) = move_file_no_replace(&final_destination, source) {
            return Err(format!(
                "Failed to write rename manifest: {}. Could not restore the original path: {}. The file remains at {}",
                manifest_error, rollback_error, final_destination.display()
            ));
        }
        remove_xattr_original_name(source);
        return Err(format!("Failed to write rename manifest; rename was rolled back: {}", manifest_error));
    }
    Ok(final_destination.to_string_lossy().to_string())
}

fn restore_file_no_replace(path: &Path, target: &Path) -> Result<(), String> {
    match move_file_no_replace(path, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
            "Cannot restore original filename because it already exists: {}", target.display()
        )),
        Err(error) => Err(format!(
            "Failed to restore filename: {}. If the target was created, both paths may remain: {} and {}",
            error, path.display(), target.display()
        )),
    }
}

/// Restores a regular file using its original-name xattr or a verified history mapping.
#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) async fn undo_rename_file(file_path: String) -> Result<UndoResult, String> {
    let path = Path::new(&file_path);
    require_regular_file(path)?;
    let parent = path.parent().ok_or_else(|| "File has no parent directory".to_string())?;
    let _lock = lock_directory(parent)?;
    require_regular_file(path)?;
    let current_name = path.file_name().and_then(|name| name.to_str())
        .ok_or_else(|| "File has no filename".to_string())?.to_string();

    if let Some(original_name) = get_xattr_original_name(path) {
        if !original_name.is_empty() && original_name != current_name {
            validate_original_name(&original_name)?;
            let target = parent.join(&original_name);
            restore_file_no_replace(path, &target)?;
            remove_xattr_original_name(&target);
            return Ok(UndoResult {
                restored_path: target.to_string_lossy().to_string(),
                original_name,
                source: if cfg!(windows) { "ads" } else { "xattr" }.to_string(),
            });
        }
    }

    let mut manifests = Vec::new();
    for entry in fs::read_dir(parent).map_err(|error| format!("Failed to read rename folder: {}", error))? {
        let entry = entry.map_err(|error| format!("Failed to read rename folder: {}", error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("rename_history_") && name.ends_with(".json") {
            manifests.push(entry.path());
        }
    }
    manifests.sort_unstable_by(|left, right| right.cmp(left));
    for manifest_path in manifests {
        let metadata = fs::symlink_metadata(&manifest_path)
            .map_err(|error| format!("Failed to inspect rename manifest: {}", error))?;
        if !metadata.file_type().is_file() {
            return Err(format!("Rename manifest is not a regular file: {}", manifest_path.display()));
        }
        let content = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Failed to read rename manifest: {}", error))?;
        let manifest: RenameHistoryManifest = serde_json::from_str(&content)
            .map_err(|error| format!("Failed to parse rename manifest: {}", error))?;
        if manifest.version != 1 {
            return Err(format!("Unsupported rename manifest version: {}", manifest.version));
        }
        if let Some(mapping) = manifest.mappings.iter().find(|mapping| mapping.current_name == current_name) {
            validate_original_name(&mapping.original_name)?;
            if mapping.original_name == current_name {
                return Err("Rename history contains the current filename as its original name".to_string());
            }
            let actual_hash = compute_file_sha256(path)?;
            if mapping.sha256.len() != 64 || !mapping.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Rename history contains an invalid SHA-256 hash".to_string());
            }
            if !mapping.sha256.eq_ignore_ascii_case(&actual_hash) {
                return Err("Cannot restore filename: SHA-256 does not match rename history".to_string());
            }
            let target = parent.join(&mapping.original_name);
            restore_file_no_replace(path, &target)?;
            remove_xattr_original_name(&target);
            return Ok(UndoResult {
                restored_path: target.to_string_lossy().to_string(),
                original_name: mapping.original_name.clone(),
                source: "manifest".to_string(),
            });
        }
    }
    Err("No original filename record found in filesystem metadata or rename_history manifests".to_string())
}

#[cfg(test)]
#[path = "tests/rename.rs"]
mod tests;
