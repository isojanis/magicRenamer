use crate::naming::find_document_date;
use office_oxide::Document;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiscoveredFile {
    pub name: String,
    pub path: String,
    pub extension: String,
    pub size_bytes: u64,
}

fn is_app_data_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false; };
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "config.json" | "config.local.json")
        || (name.starts_with("rename_history_") && name.ends_with(".json"))
}

/// Recursively scans a directory for supported document formats.
#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn scan_folder(folder_path: String) -> Result<Vec<DiscoveredFile>, String> {
    let p = Path::new(&folder_path);
    if !p.exists() || !p.is_dir() {
        return Err(format!("Directory does not exist or is not a folder: {}", folder_path));
    }

    let supported_exts = ["pdf", "docx", "xlsx", "doc", "xls", "txt", "md", "csv", "json"];
    let mut files = Vec::new();

    for entry in WalkDir::new(p).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path();
            if is_app_data_file(path) { continue; }
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
pub(crate) async fn extract_document_snippet(file_path: String) -> Result<String, String> {
    let path = Path::new(&file_path);
    if is_app_data_file(path) {
        return Err("Application settings and rename history cannot be processed as documents.".into());
    }
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

    let mut snippet = smart_excerpt(&ext, &full_text);
    let original_name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
    if let Some(date) = find_document_date(&full_text, original_name) {
        if !snippet.contains(&date) {
            snippet = format!("Document date: {}\n{}", date, snippet);
        }
    }

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

#[cfg(test)]
#[path = "tests/documents.rs"]
mod tests;
