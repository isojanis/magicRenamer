use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Default)]
pub(crate) struct AuthorizedFolder(RwLock<Option<PathBuf>>);

impl AuthorizedFolder {
    fn set(&self, folder: &Path) -> Result<String, String> {
        let canonical = folder.canonicalize().map_err(|error| format!("Could not open selected folder: {error}"))?;
        if !canonical.is_dir() { return Err("Select a folder, not a file.".into()); }
        *self.0.write().map_err(|_| "Folder authorization is unavailable.")? = Some(canonical.clone());
        Ok(canonical.to_string_lossy().into_owned())
    }

    fn root(&self) -> Result<PathBuf, String> {
        self.0.read().map_err(|_| "Folder authorization is unavailable.".to_string())?
            .clone().ok_or_else(|| "Select a folder before processing documents.".to_string())
    }

    fn check_existing(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let root = self.root()?;
        let canonical = path.as_ref().canonicalize()
            .map_err(|error| format!("Could not inspect requested path: {error}"))?;
        if canonical.starts_with(root) { Ok(()) } else { Err("Path is outside the selected folder.".into()) }
    }

    fn check_new_file(&self, path: &str) -> Result<(), String> {
        let destination = Path::new(path);
        let parent = destination.parent().ok_or("Destination needs a parent folder.")?;
        self.check_existing(parent)?;
        if destination.exists() { self.check_existing(path)?; }
        Ok(())
    }
}

#[cfg(feature = "desktop")]
mod commands {
    use super::AuthorizedFolder;
    use crate::{ai, config, documents, naming, rename};
    use tauri::{AppHandle, State};
    use tauri_plugin_dialog::DialogExt;

    #[tauri::command]
    pub(crate) async fn select_folder(app: AppHandle, folder: State<'_, AuthorizedFolder>) -> Result<Option<String>, String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.dialog().file().set_title("Select Folder to Triage").pick_folder(move |selected| {
            let _ = sender.send(selected);
        });
        let selected = receiver.await.map_err(|_| "Folder dialog closed unexpectedly.")?;
        selected.map(|path| {
            let path = path.into_path().map_err(|_| "Selected folder is not a local path.")?;
            folder.set(&path)
        }).transpose()
    }

    #[tauri::command]
    pub(crate) async fn import_previous_settings(app: AppHandle) -> Result<Option<config::PublicPromptConfig>, String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.dialog().file().set_title("Import MagicRenamer settings").add_filter("JSON settings", &["json"]).pick_file(move |selected| {
            let _ = sender.send(selected);
        });
        let selected = receiver.await.map_err(|_| "Settings dialog closed unexpectedly.")?;
        selected.map(|path| {
            let path = path.into_path().map_err(|_| "Selected settings file is not a local path.")?;
            config::import_prompt_config(path.to_string_lossy().into_owned())
        }).transpose()
    }

    #[tauri::command]
    pub(crate) fn scan_folder(folder_path: String, folder: State<'_, AuthorizedFolder>) -> Result<Vec<documents::DiscoveredFile>, String> {
        folder.check_existing(&folder_path)?;
        documents::scan_folder(folder_path)
    }

    #[tauri::command]
    pub(crate) fn build_directory_blueprint(folder_path: String, folder: State<'_, AuthorizedFolder>) -> Result<String, String> {
        folder.check_existing(&folder_path)?;
        naming::build_directory_blueprint(folder_path)
    }

    #[tauri::command]
    pub(crate) async fn extract_document_snippet(file_path: String, folder: State<'_, AuthorizedFolder>) -> Result<String, String> {
        folder.check_existing(&file_path)?;
        documents::extract_document_snippet(file_path).await
    }

    #[tauri::command]
    pub(crate) async fn suggest_filename_with_ollama(
        text_snippet: String, extension: String, model: Option<String>, folder_context: Option<String>,
        file_path: String, processing_run_id: Option<u64>, folder: State<'_, AuthorizedFolder>,
    ) -> Result<naming::FilenameSuggestion, String> {
        folder.check_existing(&file_path)?;
        ai::suggest_filename_with_ollama(text_snippet, extension, model, folder_context, file_path, processing_run_id).await
    }

    #[tauri::command]
    pub(crate) async fn rename_file(old_path: String, new_path: String, folder: State<'_, AuthorizedFolder>) -> Result<String, String> {
        folder.check_existing(&old_path)?;
        folder.check_new_file(&new_path)?;
        rename::rename_file(old_path, new_path).await
    }

    #[tauri::command]
    pub(crate) async fn undo_rename_file(file_path: String, folder: State<'_, AuthorizedFolder>) -> Result<rename::UndoResult, String> {
        folder.check_existing(&file_path)?;
        rename::undo_rename_file(file_path).await
    }
}

#[cfg(feature = "desktop")]
pub(crate) use commands::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_temp_dir;
    use std::fs;

    #[test]
    fn rejects_paths_outside_selected_folder_including_symlink_aliases() {
        let root = unique_temp_dir("authorized-root");
        let outside = unique_temp_dir("authorized-outside");
        fs::write(root.join("inside.txt"), "inside").unwrap();
        fs::write(outside.join("outside.txt"), "outside").unwrap();
        let authorized = AuthorizedFolder::default();
        assert!(authorized.check_existing(root.to_str().unwrap()).is_err());
        authorized.set(&root).unwrap();
        assert!(authorized.check_existing(root.join("inside.txt").to_str().unwrap()).is_ok());
        assert!(authorized.check_existing(outside.join("outside.txt").to_str().unwrap()).is_err());
        assert!(authorized.check_new_file(outside.join("new.txt").to_str().unwrap()).is_err());
        assert!(authorized.check_new_file(root.join("new.txt").to_str().unwrap()).is_ok());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&outside, root.join("alias")).unwrap();
            assert!(authorized.check_existing(root.join("alias/outside.txt").to_str().unwrap()).is_err());
            assert!(authorized.check_new_file(root.join("alias/new.txt").to_str().unwrap()).is_err());
        }
    }
}
