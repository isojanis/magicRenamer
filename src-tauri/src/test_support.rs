use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri should be inside the project root")
        .join("test_docs")
}

pub(crate) fn fixture(relative_path: &str) -> String {
    fixtures_dir().join(relative_path).to_string_lossy().into_owned()
}

pub(crate) struct TestDirectory(PathBuf);

impl std::ops::Deref for TestDirectory {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for TestDirectory {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn unique_temp_dir(label: &str) -> TestDirectory {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after UNIX epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "magicrenamer-{}-{}-{}",
        label,
        std::process::id(),
        nonce
    ));
    fs::create_dir_all(&path).expect("temporary directory should be created");
    TestDirectory(path)
}

pub(crate) fn remove_temp_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}
