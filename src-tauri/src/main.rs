mod ai;
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            documents::scan_folder,
            naming::build_directory_blueprint,
            documents::extract_document_snippet,
            ai::suggest_filename_with_ollama,
            ai::begin_processing_run,
            ai::cancel_processing_run,
            ai::test_ai_provider,
            rename::rename_file,
            rename::undo_rename_file,
            config::get_prompt_config,
            config::save_prompt_config,
            config::import_prompt_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
