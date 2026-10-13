use office_oxide::Document;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Serialize)]
struct OllamaGenerateRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiscoveredFile {
    pub name: String,
    pub path: String,
    pub extension: String,
    pub size_bytes: u64,
}

/// Recursively scans a directory for supported document formats (pdf, docx, xlsx, doc, xls, txt, md, csv)
#[tauri::command]
fn scan_folder(folder_path: String) -> Result<Vec<DiscoveredFile>, String> {
    let p = Path::new(&folder_path);
    if !p.exists() || !p.is_dir() {
        return Err(format!("Directory does not exist or is not a folder: {}", folder_path));
    }

    let supported_exts = ["pdf", "docx", "xlsx", "doc", "xls", "txt", "md", "csv"];
    let mut files = Vec::new();

    for entry in WalkDir::new(p).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) {
                if supported_exts.contains(&ext.as_str()) {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push(DiscoveredFile {
                        name: entry.file_name().to_string_lossy().to_string(),
                        path: path.to_string_lossy().to_string(),
                        extension: ext,
                        size_bytes: size,
                    });
                }
            }
        }
    }

    Ok(files)
}

/// 1. Determines file extension, extracts text using pdf-extract, office_oxide, or std::fs,
/// and truncates it to 500-1000 characters.
#[tauri::command]
async fn extract_document_snippet(file_path: String) -> Result<String, String> {
    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(format!("File does not exist: {}", file_path));
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let full_text = match ext.as_str() {
        "pdf" => {
            pdf_extract::extract_text(path).map_err(|e| format!("Failed to extract PDF text: {}", e))?
        }
        "docx" | "xlsx" | "doc" | "xls" => {
            let doc = Document::open(path)
                .map_err(|e| format!("Failed to open Office document: {}", e))?;
            doc.plain_text()
        }
        "txt" | "md" | "csv" => {
            fs::read_to_string(path).map_err(|e| format!("Failed to read text file: {}", e))?
        }
        _ => return Err(format!("Unsupported file extension: .{}", ext)),
    };

    // Clean whitespace and truncate snippet to max 1000 chars
    let trimmed = full_text.trim();
    let snippet: String = trimmed.chars().take(1000).collect();

    if snippet.is_empty() {
        Err("No readable text found in document".into())
    } else {
        Ok(snippet)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub prompt_template: String,
    pub model: String,
    pub temperature: f32,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prompt: "You are a specialized file-naming assistant. Analyze the document snippet and return ONLY a single standardized filename in format: YYYY-MM-DD_Topic.ext (or Topic.ext if no date found). Do not include quotes, explanation, or markdown. Use alphanumeric characters, hyphens, and underscores only.".to_string(),
            prompt_template: "Target extension: .{extension}\nDocument content snippet:\n\"\"\"\n{text_snippet}\n\"\"\"\nSuggested filename:".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            temperature: 0.2,
        }
    }
}

fn get_config_path() -> std::path::PathBuf {
    // Check current working directory, then relative to exe, fallback to default in pwd
    let local = Path::new("config.json");
    if local.exists() {
        return local.to_path_buf();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("config.json");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    local.to_path_buf()
}

fn load_prompt_config() -> PromptConfig {
    let path = get_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<PromptConfig>(&content) {
            return cfg;
        }
    }
    let default_cfg = PromptConfig::default();
    let _ = fs::write(&path, serde_json::to_string_pretty(&default_cfg).unwrap_or_default());
    default_cfg
}

#[tauri::command]
fn get_prompt_config() -> Result<PromptConfig, String> {
    Ok(load_prompt_config())
}

#[tauri::command]
fn save_prompt_config(config: PromptConfig) -> Result<(), String> {
    let path = get_config_path();
    let data = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(&path, data).map_err(|e| format!("Failed to write config file to {:?}: {}", path, e))?;
    Ok(())
}

/// 2. Suggests a standardized filename (e.g. YYYY-MM-DD_Topic.extension)
/// by contacting local Ollama instance (localhost:11434). Uses configuration from config.json.
#[tauri::command]
async fn suggest_filename_with_ollama(
    text_snippet: String,
    extension: String,
    model: Option<String>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let config = load_prompt_config();

    let selected_model = model.unwrap_or(config.model);
    let system_prompt = config.system_prompt;

    // Substitute placeholders {extension} and {text_snippet} in prompt_template
    let prompt = config
        .prompt_template
        .replace("{extension}", &extension)
        .replace("{text_snippet}", &text_snippet);

    let payload = OllamaGenerateRequest {
        model: selected_model,
        prompt,
        system: system_prompt,
        stream: false,
    };

    let response = client
        .post("http://localhost:11434/api/generate")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama (http://localhost:11434): {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Ollama API returned error: {}", response.status()));
    }

    let parsed: OllamaGenerateResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

    let mut suggested_name = parsed
        .response
        .trim()
        .trim_matches(|c| c == '`' || c == '"' || c == '\'')
        .trim()
        .to_string();

    // Sanitize in case model included newlines or paths
    if let Some(first_line) = suggested_name.lines().next() {
        suggested_name = first_line.trim().to_string();
    }

    // Ensure extension matches
    let target_ext = format!(".{}", extension);
    if !suggested_name.to_lowercase().ends_with(&target_ext.to_lowercase()) {
        suggested_name = format!("{}{}", suggested_name, target_ext);
    }

    Ok(suggested_name)
}

/// 3. Safely renames the file on the local filesystem with collision handling.
#[tauri::command]
async fn rename_file(old_path: String, new_path: String) -> Result<String, String> {
    let source = Path::new(&old_path);
    if !source.exists() {
        return Err(format!("Source file does not exist: {}", old_path));
    }

    let dest = Path::new(&new_path);
    let mut final_dest = dest.to_path_buf();

    // If destination file already exists and isn't the same file, append a counter suffix (e.g., _1, _2)
    if final_dest.exists() && final_dest != source {
        let parent = final_dest.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let stem = final_dest
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = final_dest
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{}", s))
            .unwrap_or_default();

        let mut counter = 1;
        loop {
            let candidate_name = format!("{}_{}{}", stem, counter, ext);
            let candidate_path = parent.join(candidate_name);
            if !candidate_path.exists() {
                final_dest = candidate_path;
                break;
            }
            counter += 1;
        }
    }

    fs::rename(source, &final_dest).map_err(|e| format!("Failed to rename file: {}", e))?;
    Ok(final_dest.to_string_lossy().to_string())
}

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
            scan_folder,
            extract_document_snippet,
            suggest_filename_with_ollama,
            rename_file,
            get_prompt_config,
            save_prompt_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
