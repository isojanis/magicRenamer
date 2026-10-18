mod ai;
mod authorized_folder;
mod config;
mod documents;
mod naming;
mod rename;

#[cfg(test)]
mod test_support;

#[cfg(feature = "desktop")]
fn main() {
    // Workaround for WebKitGTK DMA-BUF renderer protocol error on Wayland (e.g. KWin / GNOME with NVIDIA/Mesa)
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if std::env::var("__NV_DISABLE_EXPLICIT_SYNC").is_err() {
            std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(authorized_folder::AuthorizedFolder::default())
        .invoke_handler(tauri::generate_handler![
            authorized_folder::select_folder,
            authorized_folder::import_previous_settings,
            authorized_folder::scan_folder,
            authorized_folder::build_directory_blueprint,
            authorized_folder::extract_document_snippet,
            authorized_folder::suggest_filename_with_ollama,
            ai::begin_processing_run,
            ai::cancel_processing_run,
            ai::test_ai_provider,
            authorized_folder::rename_file,
            authorized_folder::undo_rename_file,
            config::get_prompt_config,
            config::save_prompt_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
