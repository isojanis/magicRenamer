use office_oxide::Document;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
#[cfg_attr(feature = "desktop", tauri::command)]
fn scan_folder(folder_path: String) -> Result<Vec<DiscoveredFile>, String> {
    let p = Path::new(&folder_path);
    if !p.exists() || !p.is_dir() {
        return Err(format!("Directory does not exist or is not a folder: {}", folder_path));
    }

    let supported_exts = ["pdf", "docx", "xlsx", "doc", "xls", "txt", "md", "csv", "json"];
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
#[cfg_attr(feature = "desktop", tauri::command)]
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
        "txt" | "md" | "csv" | "json" => {
            fs::read_to_string(path).map_err(|e| format!("Failed to read text file: {}", e))?
        }
        _ => return Err(format!("Unsupported file extension: .{}", ext)),
    };

    let snippet = smart_excerpt(&ext, &full_text);

    if snippet.is_empty() {
        Err("No readable text found in document".into())
    } else {
        Ok(snippet)
    }
}

fn smart_excerpt(extension: &str, text: &str) -> String {
    let trimmed = text.trim();
    let excerpt = match extension {
        "json" => serde_json::from_str::<serde_json::Value>(trimmed).ok().map(|value| {
            let mut summary = Vec::new();
            match &value {
                serde_json::Value::Object(map) => {
                    summary.push(format!("Root keys: {}", map.keys().take(40).cloned().collect::<Vec<_>>().join(", ")));
                    for (key, item) in map.iter().take(10) {
                        if let Some(array) = item.as_array() {
                            summary.push(format!("{}: array with {} items", key, array.len()));
                        } else if item.is_string() || item.is_number() || item.is_boolean() {
                            summary.push(format!("{}: {}", key, item));
                        }
                    }
                }
                serde_json::Value::Array(items) => summary.push(format!("Root array with {} items", items.len())),
                _ => summary.push(format!("Root value: {}", value)),
            }
            summary.join("\n")
        }).unwrap_or_else(|| trimmed.chars().take(1000).collect()),
        "csv" => trimmed.lines().take(3).collect::<Vec<_>>().join("\n"),
        "pdf" | "docx" | "doc" | "xlsx" | "xls" => trimmed.lines().filter(|line| !line.trim().is_empty()).take(8).collect::<Vec<_>>().join("\n"),
        "txt" | "md" => {
            let lines: Vec<&str> = trimmed.lines().collect();
            let mut selected = Vec::new();
            for line in lines.iter().take(3) { selected.push(*line); }
            for line in &lines {
                if line.trim_start().starts_with('#') && !selected.contains(line) { selected.push(*line); }
            }
            if let Some(last) = lines.last() {
                if !selected.contains(last) { selected.push(*last); }
            }
            selected.join("\n")
        }
        _ => trimmed.chars().take(1000).collect(),
    };
    excerpt.chars().take(1400).collect::<String>()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub prompt_template: String,
    pub model: String,
    pub temperature: f32,
    #[serde(default = "default_folder_context_enabled")]
    pub folder_context_enabled: bool,
}

fn default_folder_context_enabled() -> bool { true }

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prompt: "Use the document and directory context to suggest a concise filename that follows the directory's conventions. Return only valid JSON with string fields new_name, category, and reason. new_name must be a filename, not a path, and retain the target extension.".to_string(),
            prompt_template: "Directory context:\n{folder_context}\n\nTarget extension: .{extension}\nDocument excerpt:\n\"\"\"\n{text_snippet}\n\"\"\"\nReturn JSON with new_name, category, and reason.".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            temperature: 0.2,
            folder_context_enabled: true,
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

#[cfg_attr(feature = "desktop", tauri::command)]
fn get_prompt_config() -> Result<PromptConfig, String> {
    Ok(load_prompt_config())
}

#[cfg_attr(feature = "desktop", tauri::command)]
fn save_prompt_config(config: PromptConfig) -> Result<(), String> {
    let path = get_config_path();
    let data = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(&path, data).map_err(|e| format!("Failed to write config file to {:?}: {}", path, e))?;
    Ok(())
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FilenameSuggestion {
    pub new_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub reason: String,
}

/// Creates a compact, local-only directory blueprint for the selected folder.
#[cfg_attr(feature = "desktop", tauri::command)]
fn build_directory_blueprint(folder_path: String) -> Result<String, String> {
    let root = Path::new(&folder_path);
    if !root.is_dir() {
        return Err(format!("Directory does not exist or is not a folder: {}", folder_path));
    }
    let mut names = Vec::new();
    let mut history_paths = Vec::new();
    let mut extension_counts: HashMap<String, usize> = HashMap::new();
    for entry in fs::read_dir(root).map_err(|e| format!("Failed to read directory: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        if !path.is_file() { continue; }
        if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
            *extension_counts.entry(ext.to_ascii_lowercase()).or_default() += 1;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("rename_history_") && name.ends_with(".json") {
            history_paths.push(path);
        } else if !name.starts_with('.') {
            names.push(name);
        }
    }
    names.sort_by_key(|name| name.to_lowercase());
    names.truncate(20);

    let mut categories: HashMap<String, usize> = HashMap::new();
    let mut prior_names = Vec::new();
    for path in history_paths {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(mappings) = value.get("mappings").and_then(|v| v.as_array()) {
                    for mapping in mappings {
                        if let Some(name) = mapping.get("current_name").and_then(|v| v.as_str()) {
                            prior_names.push(name.to_string());
                        }
                        if let Some(category) = mapping.get("category").and_then(|v| v.as_str()) {
                            *categories.entry(category.to_string()).or_default() += 1;
                        }
                    }
                }
            }
        }
    }

    let has_dates = names.iter().any(|name| {
        let bytes = name.as_bytes();
        bytes.len() >= 10 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'-'
    });
    let has_underscores = names.iter().any(|name| name.contains('_'));
    let has_hyphens = names.iter().any(|name| name.contains('-'));
    let has_numbered_suffix = names.iter().any(|name| {
        Path::new(name).file_stem().and_then(|stem| stem.to_str())
            .and_then(|stem| stem.rsplit(|c: char| !c.is_ascii_digit()).next())
            .map(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && digits.len() <= 4)
            .unwrap_or(false)
    });
    let has_title_case = names.iter().any(|name| {
        Path::new(name).file_stem().and_then(|stem| stem.to_str()).map(|stem| {
            let words: Vec<_> = stem.split(|c: char| c == '_' || c == '-' || c.is_whitespace()).filter(|word| !word.is_empty()).collect();
            words.len() > 1 && words.iter().all(|word| word.chars().next().map(char::is_uppercase).unwrap_or(false))
        }).unwrap_or(false)
    });
    let mut lines = vec![format!("Parent folder: {}", root.file_name().and_then(|n| n.to_str()).unwrap_or("(root)"))];
    let mut patterns = Vec::new();
    if has_dates { patterns.push("YYYY-MM-DD date prefix"); }
    if has_underscores { patterns.push("underscore-separated words"); }
    if has_hyphens { patterns.push("hyphen-separated words"); }
    if has_title_case { patterns.push("TitleCase words"); }
    if has_numbered_suffix { patterns.push("numeric suffix / sequence numbering"); }
    if !patterns.is_empty() { lines.push(format!("Observed conventions: {}", patterns.join(", "))); }
    if !extension_counts.is_empty() {
        let mut extensions: Vec<_> = extension_counts.into_iter().collect();
        extensions.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        lines.push(format!("Common extensions: {}", extensions.iter().take(8).map(|(ext, count)| format!(".{} ({})", ext, count)).collect::<Vec<_>>().join(", ")));
    }
    if !names.is_empty() { lines.push(format!("Sibling filenames: {}", names.join(" | "))); }
    prior_names.sort();
    prior_names.dedup();
    prior_names.truncate(5);
    if !prior_names.is_empty() { lines.push(format!("Prior rename examples: {}", prior_names.join(" | "))); }
    if !categories.is_empty() {
        let mut categories: Vec<_> = categories.into_iter().collect();
        categories.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        lines.push(format!("Prior categories: {}", categories.iter().take(8).map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    Ok(lines.join("\n").chars().take(900).collect())
}

/// 2. Suggests a standardized filename (e.g. YYYY-MM-DD_Topic.extension)
/// by contacting local Ollama instance (localhost:11434). Uses configuration from config.json.
#[cfg_attr(feature = "desktop", tauri::command)]
async fn suggest_filename_with_ollama(
    text_snippet: String,
    extension: String,
    model: Option<String>,
    folder_context: Option<String>,
) -> Result<FilenameSuggestion, String> {
    let client = reqwest::Client::new();
    let config = load_prompt_config();

    let selected_model = model.unwrap_or(config.model.clone());
    let context_enabled = config.folder_context_enabled && folder_context.is_some();
    let system_prompt = if context_enabled {
        format!("{}\n\nUse the directory context when choosing the name. Return only valid JSON with string fields new_name, category, and reason.", config.system_prompt)
    } else {
        config.system_prompt.clone()
    };

    let mut prompt = config
        .prompt_template
        .replace("{extension}", &extension)
        .replace("{text_snippet}", &text_snippet)
        .replace("{folder_context}", folder_context.as_deref().unwrap_or("(disabled)"));
    if context_enabled && !config.prompt_template.contains("{folder_context}") {
        prompt = format!("Directory context:\n{}\n\n{}", folder_context.as_deref().unwrap_or_default(), prompt);
    }

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

    let response_text = parsed.response.trim().trim_matches('`').trim();
    let suggestion = serde_json::from_str::<FilenameSuggestion>(response_text).ok().or_else(|| {
        let start = response_text.find('{')?;
        let end = response_text.rfind('}')?;
        serde_json::from_str::<FilenameSuggestion>(&response_text[start..=end]).ok()
    }).or_else(|| {
        let plain = response_text.lines().next()?.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if plain.is_empty() { None } else { Some(FilenameSuggestion { new_name: plain.to_string(), category: String::new(), reason: String::new() }) }
    }).ok_or_else(|| "Ollama returned an empty filename suggestion.".to_string())?;

    let mut suggested_name = suggestion.new_name.lines().next().unwrap_or("").trim().to_string();
    if suggested_name.is_empty() || suggested_name.contains('/') || suggested_name.contains('\\') || suggested_name == "." || suggested_name == ".." {
        return Err("Ollama returned an invalid filename.".into());
    }
    let target_ext = format!(".{}", extension);
    if !suggested_name.to_lowercase().ends_with(&target_ext.to_lowercase()) {
        suggested_name = format!("{}{}", suggested_name, target_ext);
    }
    Ok(FilenameSuggestion { new_name: suggested_name, ..suggestion })
}

/// 3. Safely renames the file on the local filesystem with collision handling.
#[cfg_attr(feature = "desktop", tauri::command)]
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

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri should be inside the project root")
            .join("test_docs")
    }

    fn fixture(relative_path: &str) -> String {
        fixtures_dir().join(relative_path).to_string_lossy().into_owned()
    }

    #[test]
    fn recursively_scans_the_large_nested_fixture_batch() {
        let files = scan_folder(fixture("large-batch")).expect("fixture folder should scan");
        assert_eq!(files.len(), 522, "the batch has 504 root files and 18 nested files");
        assert!(files.iter().any(|file| file.name.ends_with(".json")));
        assert!(files.iter().any(|file| file.name.ends_with(".csv")));
        assert!(files.iter().any(|file| file.path.contains("/archive/")));
    }

    #[test]
    fn blueprint_uses_existing_folder_names_and_conventions() {
        let blueprint = build_directory_blueprint(fixture("finance/invoices"))
            .expect("invoice folder should produce a blueprint");
        assert!(blueprint.contains("Parent folder: invoices"));
        assert!(blueprint.contains("YYYY-MM-DD date prefix"));
        assert!(blueprint.contains("underscore-separated words"));
        assert!(blueprint.contains("2026-08-01_northstar.txt"));
        assert!(blueprint.contains("Common extensions:"));
    }

    #[test]
    fn scans_all_supported_fixture_formats_recursively() {
        let files = scan_folder(fixtures_dir().to_string_lossy().into_owned())
            .expect("test_docs should scan recursively");
        assert!(files.len() > 550, "expected the broad fixture corpus, got {} files", files.len());
        for extension in ["txt", "md", "csv", "json", "pdf", "docx"] {
            assert!(files.iter().any(|file| file.extension == extension), "missing .{} fixture", extension);
        }
    }

    #[tokio::test]
    async fn extracts_snippets_from_real_fixture_files() {
        let examples = [
            ("finance/invoices/2026-10-05_Redwood_Invoice.json", "Root keys:"),
            ("finance/invoices/2026-09-02_Harbor_Invoice_772.csv", "invoice,date,vendor,total,currency"),
            ("engineering/meeting-notes/2026-08-05_platform_sync.md", "Platform Sync"),
            ("download_1.txt", "INV-2026-0812"),
            ("document-formats/2026-08-16_Lab_Results.pdf", "Laboratory Results"),
            ("document-formats/2026-09-06_Project_Brief.docx", "Project Brief: Offline Archive"),
        ];

        for (relative_path, expected_text) in examples {
            let snippet = extract_document_snippet(fixture(relative_path))
                .await
                .unwrap_or_else(|error| panic!("failed to extract {}: {}", relative_path, error));
            assert!(snippet.contains(expected_text), "{} did not contain {:?}: {}", relative_path, expected_text, snippet);
        }
    }
}

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
            scan_folder,
            build_directory_blueprint,
            extract_document_snippet,
            suggest_filename_with_ollama,
            rename_file,
            get_prompt_config,
            save_prompt_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
