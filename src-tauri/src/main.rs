use chrono::Utc;
use office_oxide::Document;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use walkdir::WalkDir;

#[derive(Serialize)]
struct OllamaGenerateRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
    format: serde_json::Value,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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

fn load_prompt_config_at(path: &Path) -> PromptConfig {
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(cfg) = serde_json::from_str::<PromptConfig>(&content) {
            return cfg;
        }
    }
    let default_cfg = PromptConfig::default();
    let _ = fs::write(path, serde_json::to_string_pretty(&default_cfg).unwrap_or_default());
    default_cfg
}

fn load_prompt_config() -> PromptConfig {
    load_prompt_config_at(&get_config_path())
}

#[cfg_attr(feature = "desktop", tauri::command)]
fn get_prompt_config() -> Result<PromptConfig, String> {
    Ok(load_prompt_config())
}

fn save_prompt_config_at(path: &Path, config: &PromptConfig) -> Result<(), String> {
    let data = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(path, data).map_err(|e| format!("Failed to write config file to {:?}: {}", path, e))?;
    Ok(())
}

#[cfg_attr(feature = "desktop", tauri::command)]
fn save_prompt_config(config: PromptConfig) -> Result<(), String> {
    save_prompt_config_at(&get_config_path(), &config)
}

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


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FilenameSuggestion {
    pub new_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub reason: String,
}

/// Summarizes the selected folder without assuming its files share a document type.
#[cfg_attr(feature = "desktop", tauri::command)]
fn build_directory_blueprint(folder_path: String) -> Result<String, String> {
    let root = Path::new(&folder_path);
    if !root.is_dir() {
        return Err(format!("Directory does not exist or is not a folder: {}", folder_path));
    }
    let mut visible_files = 0usize;
    let mut date_prefixed = 0usize;
    for entry in fs::read_dir(root).map_err(|error| format!("Failed to read directory: {}", error))? {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {}", error))?;
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with("rename_history_") {
            continue;
        }
        visible_files += 1;
        if has_iso_date_prefix(&name) {
            date_prefixed += 1;
        }
    }
    Ok(format!(
        "Selected folder: {}. {} visible files; {} already start with YYYY-MM-DD. Files in this folder may be unrelated. Use each target document to determine its own date, subject, and type. Matching sibling names are supplied separately for that target.",
        root.file_name().and_then(|name| name.to_str()).unwrap_or("(root)"),
        visible_files,
        date_prefixed
    ))
}

fn has_iso_date_prefix(name: &str) -> bool {
    let stem = Path::new(name).file_stem().and_then(|value| value.to_str()).unwrap_or(name);
    let bytes = stem.as_bytes();
    bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && (bytes.len() == 10 || matches!(bytes[10], b'_' | b'-' | b' '))
}

fn context_tokens(text: &str) -> HashSet<String> {
    const COMMON: &[&str] = &[
        "invoice", "lasku", "receipt", "report", "document", "download", "scan",
        "untitled", "date", "due", "number", "total", "paid", "file", "the",
        "and", "from", "with", "pdf", "docx", "ry", "eur", "reference",
    ];
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.chars().count() >= 3 && !token.chars().all(char::is_numeric))
        .map(str::to_lowercase)
        .filter(|token| !COMMON.contains(&token.as_str()))
        .collect()
}

fn document_kind(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.contains("invoice") || lower.contains("lasku") {
        Some("invoice")
    } else if lower.contains("receipt") || lower.contains("kuitti") {
        Some("receipt")
    } else if lower.contains("contract") || lower.contains("agreement") || lower.contains("sopimus") {
        Some("contract")
    } else if lower.contains("report") || lower.contains("raportti") {
        Some("report")
    } else if lower.contains("meeting") || lower.contains("minutes") || lower.contains("muistio") {
        Some("meeting")
    } else {
        None
    }
}

fn relevant_sibling_names(file_path: &Path, snippet: &str) -> Vec<String> {
    let Some(parent) = file_path.parent() else { return Vec::new(); };
    let original_name = file_path.file_name().and_then(|value| value.to_str()).unwrap_or("");
    let Some(target_kind) = document_kind(snippet).or_else(|| document_kind(original_name)) else {
        return Vec::new();
    };
    let mut target_tokens = context_tokens(snippet);
    target_tokens.extend(context_tokens(original_name));
    let Ok(entries) = fs::read_dir(parent) else { return Vec::new(); };
    let mut matches = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == original_name || name.starts_with('.') || name.starts_with("rename_history_") {
            continue;
        }
        if document_kind(&name) != Some(target_kind) {
            continue;
        }
        let sibling_tokens = context_tokens(&name);
        let overlap = sibling_tokens.intersection(&target_tokens).count();
        if overlap >= 2 {
            matches.push((overlap, name));
        }
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    matches.into_iter().take(8).map(|(_, name)| name).collect()
}

fn dominant_invoice_label(names: &[String]) -> Option<&'static str> {
    let mut finnish = 0usize;
    let mut english = 0usize;
    for name in names {
        let lower = name.to_lowercase();
        let words: Vec<_> = lower.split(|character: char| !character.is_alphanumeric()).collect();
        if words.iter().any(|word| word.ends_with("lasku")) {
            finnish += 1;
        }
        if words.contains(&"invoice") {
            english += 1;
        }
    }
    if finnish >= 2 && finnish > english {
        Some("Lasku")
    } else if english >= 2 && english > finnish {
        Some("Invoice")
    } else {
        None
    }
}

const UNIVERSAL_NAMING_INSTRUCTIONS: &str = "Name the target document from its own contents. If a reliable full document date is provided, new_name must start with YYYY-MM-DD_. Put the subject or organization before the document type when both are known. Use the full organization name from the target when available. Use underscores between fields. Do not invent a date, organization, or document type. Never return a generic name such as download, scan, file, document, invoice, or lasku alone. These requirements and the JSON response format override conflicting instructions earlier in this system prompt. Return only a JSON object with string fields new_name, category, and reason. Keep the original file extension.";
const FOLDER_CONTEXT_INSTRUCTIONS: &str = "A folder can contain unrelated documents. Use only sibling names explicitly identified as relevant to this target. Matching siblings can guide spelling and category wording for the same kind of document; never copy their dates or subjects into unrelated files. The target document determines its date, subject, and type.";

fn numeric_date_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?:(20\d{2})[-./](\d{1,2})[-./](\d{1,2})|(\d{1,2})[./-](\d{1,2})[./-](20\d{2}))").unwrap())
}

fn month_date_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)(january|february|march|april|may|june|july|august|september|october|november|december)\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(20\d{2})").unwrap())
}

fn finnish_month_date_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)(\d{1,2})\.?\s+(tammikuuta|helmikuuta|maaliskuuta|huhtikuuta|toukokuuta|kesäkuuta|heinäkuuta|elokuuta|syyskuuta|lokakuuta|marraskuuta|joulukuuta|tammikuu|helmikuu|maaliskuu|huhtikuu|toukokuu|kesäkuu|heinäkuu|elokuu|syyskuu|lokakuu|marraskuu|joulukuu)\s+(20\d{2})").unwrap())
}

fn calendar_date(year: u32, month: u32, day: u32) -> Option<String> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return None,
    };
    (day >= 1 && day <= days).then(|| format!("{:04}-{:02}-{:02}", year, month, day))
}

fn date_mentions(text: &str) -> Vec<(String, usize)> {
    let mut mentions = Vec::new();
    for capture in numeric_date_regex().captures_iter(text) {
        let Some(matched) = capture.get(0) else { continue; };
        if text[..matched.start()].chars().next_back().is_some_and(char::is_alphanumeric)
            || text[matched.end()..].chars().next().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let parts = if capture.get(1).is_some() {
            (capture.get(1), capture.get(2), capture.get(3))
        } else {
            (capture.get(6), capture.get(5), capture.get(4))
        };
        let (Some(year), Some(month), Some(day)) = parts else { continue; };
        let parsed = (
            year.as_str().parse::<u32>(),
            month.as_str().parse::<u32>(),
            day.as_str().parse::<u32>(),
        );
        if let (Ok(year), Ok(month), Ok(day)) = parsed {
            if let Some(date) = calendar_date(year, month, day) {
                mentions.push((date, matched.start()));
            }
        }
    }
    const MONTHS: [&str; 12] = [
        "january", "february", "march", "april", "may", "june",
        "july", "august", "september", "october", "november", "december",
    ];
    for capture in month_date_regex().captures_iter(text) {
        let Some(matched) = capture.get(0) else { continue; };
        if text[..matched.start()].chars().next_back().is_some_and(char::is_alphanumeric)
            || text[matched.end()..].chars().next().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let month_name = capture.get(1).unwrap().as_str().to_ascii_lowercase();
        let Some(month) = MONTHS.iter().position(|name| *name == month_name) else { continue; };
        let (Ok(day), Ok(year)) = (
            capture.get(2).unwrap().as_str().parse::<u32>(),
            capture.get(3).unwrap().as_str().parse::<u32>(),
        ) else { continue; };
        if let Some(date) = calendar_date(year, month as u32 + 1, day) {
            mentions.push((date, matched.start()));
        }
    }
    const FINNISH_MONTH_PREFIXES: [&str; 12] = [
        "tammi", "helmi", "maalis", "huhti", "touko", "kesä",
        "heinä", "elo", "syys", "loka", "marras", "joulu",
    ];
    for capture in finnish_month_date_regex().captures_iter(text) {
        let Some(matched) = capture.get(0) else { continue; };
        if text[..matched.start()].chars().next_back().is_some_and(char::is_alphanumeric)
            || text[matched.end()..].chars().next().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let month_name = capture.get(2).unwrap().as_str().to_lowercase();
        let Some(month) = FINNISH_MONTH_PREFIXES.iter().position(|name| month_name.starts_with(name)) else { continue; };
        let (Ok(day), Ok(year)) = (
            capture.get(1).unwrap().as_str().parse::<u32>(),
            capture.get(3).unwrap().as_str().parse::<u32>(),
        ) else { continue; };
        if let Some(date) = calendar_date(year, month as u32 + 1, day) {
            mentions.push((date, matched.start()));
        }
    }
    mentions.sort_by_key(|(_, position)| *position);
    mentions
}

fn date_context_priority(text: &str, position: usize) -> i8 {
    let preceding = &text[..position];
    let line_start = preceding.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let label = preceding[line_start..].chars().rev().take(80).collect::<String>()
        .chars().rev().collect::<String>().to_lowercase();
    if ["eräpäivä", "erapaiva", "due", "pay by"].iter().any(|word| label.contains(word)) {
        return -1;
    }
    if ["invoice number", "invoice no", "invoice #", "reference", "viite"].iter().any(|word| label.contains(word)) {
        return -1;
    }
    if ["invoice date", "issue date", "date of issue", "laskun päiväys",
        "laskutuspäivä", "päiväys", "pvm", "date"].iter().any(|word| label.contains(word)) {
        return 2;
    }
    0
}

fn find_document_date(text: &str, original_name: &str) -> Option<String> {
    let mentions = date_mentions(text);
    let preferred: HashSet<_> = mentions.iter()
        .filter(|(_, position)| date_context_priority(text, *position) == 2)
        .map(|(date, _)| date.clone()).collect();
    if preferred.len() == 1 {
        return preferred.into_iter().next();
    }
    let unambiguous: HashSet<_> = mentions.into_iter()
        .filter(|(_, position)| date_context_priority(text, *position) >= 0)
        .map(|(date, _)| date).collect();
    if unambiguous.len() == 1 {
        return unambiguous.into_iter().next();
    }
    let from_name: HashSet<_> = date_mentions(original_name).into_iter().map(|(date, _)| date).collect();
    if from_name.len() == 1 {
        return from_name.into_iter().next();
    }
    None
}

fn finalize_suggestion(
    mut suggestion: FilenameSuggestion,
    extension: &str,
    confirmed_date: Option<&str>,
    preferred_invoice_wording: Option<&str>,
) -> Result<FilenameSuggestion, String> {
    let proposed = suggestion.new_name.lines().next().unwrap_or("").trim()
        .trim_matches(|character| character == '"' || character == '\'' || character == char::from(96))
        .trim();
    if proposed.is_empty() || proposed.contains('/') || proposed.contains('\\') || proposed == "." || proposed == ".." {
        return Err("Ollama returned an invalid filename".into());
    }
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    let expected_suffix = format!(".{}", extension);
    let mut stem = if proposed.to_ascii_lowercase().ends_with(&expected_suffix) {
        proposed[..proposed.len() - expected_suffix.len()].to_string()
    } else {
        proposed.to_string()
    };
    if let Some((before, suffix)) = stem.rsplit_once('.') {
        if ["pdf", "docx", "xlsx", "doc", "xls", "txt", "md", "csv", "json"]
            .contains(&suffix.to_ascii_lowercase().as_str()) {
            stem = before.to_string();
        }
    }
    if confirmed_date.is_none() && !date_mentions(&stem).is_empty() {
        return Err("The suggested date could not be verified from this document".into());
    }
    let without_dates = numeric_date_regex().replace_all(&stem, "");
    let without_dates = month_date_regex().replace_all(&without_dates, "");
    let without_dates = finnish_month_date_regex().replace_all(&without_dates, "");
    let mut normalized = String::new();
    for character in without_dates.chars() {
        let character = match character {
            'ä' | 'å' => 'a',
            'Ä' | 'Å' => 'A',
            'ö' => 'o',
            'Ö' => 'O',
            _ => character,
        };
        if character.is_alphanumeric() {
            normalized.push(character);
        } else if !normalized.ends_with('_') && !normalized.is_empty() {
            normalized.push('_');
        }
    }
    let mut tokens: Vec<String> = normalized.trim_matches('_').split('_')
        .filter(|token| !token.is_empty())
        .map(|token| match token.to_lowercase().as_str() {
            "invoice" => preferred_invoice_wording.unwrap_or("Invoice").to_string(),
            "lasku" => preferred_invoice_wording.unwrap_or("Lasku").to_string(),
            "ry" => "Ry".to_string(),
            _ => token.to_string(),
        })
        .collect();
    let generic = |token: &str| {
        ["download", "scan", "untitled", "file", "document", "invoice", "lasku",
            "receipt", "report", "binary"].contains(&token.to_lowercase().as_str())
    };
    if tokens.iter().any(|token| ["download", "untitled", "binary"].contains(&token.to_lowercase().as_str())) {
        return Err("The suggestion still uses a download or placeholder name".into());
    }
    if tokens.last().is_some_and(|token| ["document", "file", "scan"].contains(&token.to_lowercase().as_str())) {
        return Err("The suggestion ends with a generic document label".into());
    }
    let meaningful_word = tokens.iter().any(|token| token.chars().any(char::is_alphabetic) && !generic(token));
    let meaningful_reference = tokens.iter().any(|token| token.chars().all(char::is_numeric) && token.len() >= 5);
    if !meaningful_word && !meaningful_reference {
        return Err("The suggestion needs a specific subject, organization, or reference number".into());
    }
    if tokens.len() > 2 && ["invoice", "lasku"].contains(&tokens[0].to_lowercase().as_str()) && meaningful_word {
        let document_type = tokens.remove(0);
        tokens.push(document_type);
    }
    if let Some(wording) = preferred_invoice_wording {
        let has_invoice_type = tokens.iter().any(|token| {
            let lower = token.to_lowercase();
            lower == "invoice" || lower.ends_with("lasku")
        });
        if !has_invoice_type {
            tokens.push(wording.to_string());
        }
    }
    let body = tokens.join("_");
    suggestion.new_name = match confirmed_date {
        Some(date) => format!("{}_{}.{}", date, body, extension),
        None => format!("{}.{}", body, extension),
    };
    Ok(suggestion)
}

fn parse_model_suggestion(response: &str) -> Result<FilenameSuggestion, String> {
    let text = response.trim().trim_matches(char::from(96)).trim();
    serde_json::from_str::<FilenameSuggestion>(text).ok().or_else(|| {
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        serde_json::from_str::<FilenameSuggestion>(&text[start..=end]).ok()
    }).or_else(|| {
        if text.starts_with('{') || text.starts_with('[') {
            return None;
        }
        let plain = text.lines().next()?.trim().trim_matches(|character| character == '"' || character == '\'').trim();
        (!plain.is_empty()).then(|| FilenameSuggestion {
            new_name: plain.to_string(),
            category: String::new(),
            reason: String::new(),
        })
    }).ok_or_else(|| "Ollama returned an empty or unreadable suggestion".to_string())
}

/// Suggests a filename for one document using its content and only matching sibling names.
#[cfg_attr(feature = "desktop", tauri::command)]
async fn suggest_filename_with_ollama(
    text_snippet: String,
    extension: String,
    model: Option<String>,
    folder_context: Option<String>,
    file_path: String,
) -> Result<FilenameSuggestion, String> {
    suggest_filename_with_ollama_using_config(
        text_snippet,
        extension,
        model,
        folder_context,
        file_path,
        load_prompt_config(),
        "http://localhost:11434/api/generate",
    ).await
}

async fn suggest_filename_with_ollama_using_config(
    text_snippet: String,
    extension: String,
    model: Option<String>,
    folder_context: Option<String>,
    file_path: String,
    config: PromptConfig,
    endpoint: &str,
) -> Result<FilenameSuggestion, String> {
    let client = reqwest::Client::new();
    let selected_model = model.unwrap_or(config.model.clone());
    let context_enabled = config.folder_context_enabled && folder_context.is_some();
    let original_name = Path::new(&file_path).file_name().and_then(|value| value.to_str()).unwrap_or("");
    let confirmed_date = find_document_date(&text_snippet, original_name);
    let system_prompt = format!(
        "{}\n\n{}{}",
        config.system_prompt,
        UNIVERSAL_NAMING_INSTRUCTIONS,
        if context_enabled { format!("\n\n{}", FOLDER_CONTEXT_INSTRUCTIONS) } else { String::new() }
    );
    let mut prompt = config.prompt_template
        .replace("{extension}", &extension)
        .replace("{text_snippet}", &text_snippet)
        .replace("{folder_context}", folder_context.as_deref().unwrap_or("(disabled)"));
    if context_enabled && !config.prompt_template.contains("{folder_context}") {
        prompt = format!("Folder context:\n{}\n\n{}", folder_context.as_deref().unwrap_or_default(), prompt);
    }
    prompt.push_str(&format!("\nOriginal filename: {}", original_name));
    if let Some(date) = confirmed_date.as_deref() {
        prompt.push_str(&format!("\nConfirmed document date: {}. Start new_name with {}_.", date, date));
    } else {
        prompt.push_str("\nNo full document date could be confirmed. Do not invent one.");
    }
    let matching_names = if context_enabled {
        relevant_sibling_names(Path::new(&file_path), &text_snippet)
    } else {
        Vec::new()
    };
    let preferred_invoice_wording = if document_kind(&text_snippet).or_else(|| document_kind(original_name)) == Some("invoice") {
        dominant_invoice_label(&matching_names)
    } else {
        None
    };
    if context_enabled {
        if matching_names.is_empty() {
            prompt.push_str("\nNo clearly related sibling names were found. Name this file from its own content.");
        } else {
            prompt.push_str(&format!("\nRelevant sibling names for this target only: {}", matching_names.join(" | ")));
        }
        if let Some(wording) = preferred_invoice_wording {
            prompt.push_str(&format!("\nUse {} as the document-type wording for matching invoices.", wording));
        }
    }
    prompt.push_str("\nReturn only JSON with new_name, category, and reason. The name must identify this specific document.");
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "new_name": {"type": "string"},
            "category": {"type": "string"},
            "reason": {"type": "string"}
        },
        "required": ["new_name", "category", "reason"]
    });
    let temperature = if config.temperature.is_finite() {
        config.temperature.clamp(0.0, 2.0)
    } else {
        0.0
    };
    let mut last_issue = String::new();
    for attempt in 0..2 {
        let payload = OllamaGenerateRequest {
            model: selected_model.clone(),
            prompt: prompt.clone(),
            system: system_prompt.clone(),
            stream: false,
            format: schema.clone(),
            options: OllamaOptions { temperature },
        };
        let response = client.post(endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|error| format!("Failed to connect to Ollama (http://localhost:11434): {}", error))?;
        if !response.status().is_success() {
            return Err(format!("Ollama API returned error: {}", response.status()));
        }
        let generated: OllamaGenerateResponse = response.json().await
            .map_err(|error| format!("Failed to parse Ollama response: {}", error))?;
        let result = parse_model_suggestion(&generated.response)
            .and_then(|suggestion| finalize_suggestion(
                suggestion, &extension, confirmed_date.as_deref(), preferred_invoice_wording
            ));
        match result {
            Ok(suggestion) => return Ok(suggestion),
            Err(issue) => {
                last_issue = issue;
                if attempt == 0 {
                    prompt.push_str(&format!(
                        "\nYour previous filename was rejected: {}. Try again with a specific subject or reference from this document. Respect the confirmed date when supplied.",
                        last_issue
                    ));
                }
            }
        }
    }
    Err(format!("Ollama could not produce a usable filename: {}", last_issue))
}

const XATTR_ORIGINAL_NAME: &str = "user.magicrenamer.original_name";

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
    pub source: String, // "xattr" or "manifest"
}

fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("Failed to open file for SHA-256: {}", e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("Failed to hash file: {}", e))?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn set_xattr_original_name(path: &Path, original_name: &str) {
    // Extended attributes are supported on Linux, macOS, and Windows NTFS
    let _ = xattr::set(path, XATTR_ORIGINAL_NAME, original_name.as_bytes());
}

fn get_xattr_original_name(path: &Path) -> Option<String> {
    xattr::get(path, XATTR_ORIGINAL_NAME)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn remove_xattr_original_name(path: &Path) {
    let _ = xattr::remove(path, XATTR_ORIGINAL_NAME);
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
async fn rename_file(old_path: String, new_path: String) -> Result<String, String> {
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
async fn undo_rename_file(file_path: String) -> Result<UndoResult, String> {
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
                source: "xattr".to_string(),
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
    Err("No original filename record found in xattr or rename_history manifests".to_string())
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

    struct TestDirectory(PathBuf);

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

    fn unique_temp_dir(label: &str) -> TestDirectory {
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

    fn remove_temp_dir(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    async fn mock_ollama(
        responses: Vec<(u16, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<serde_json::Value>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, model_response) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0u8; 4096];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert!(read > 0, "client closed before sending the request");
                    request.extend_from_slice(&chunk[..read]);
                    if let Some(position) = find_subslice(&request, b"\r\n\r\n") {
                        let end = position + 4;
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .expect("request should have a content length");
                        while request.len() < end + content_length {
                            let mut body_chunk = vec![0u8; end + content_length - request.len()];
                            let read = stream.read(&mut body_chunk).await.unwrap();
                            assert!(read > 0, "client closed before sending the request body");
                            request.extend_from_slice(&body_chunk[..read]);
                        }
                        break end;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                requests.push(
                    serde_json::from_slice(&request[header_end..header_end + content_length])
                        .unwrap(),
                );
                let body = serde_json::json!({ "response": model_response }).to_string();
                let reason = if status == 200 { "OK" } else { "Error" };
                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    reason,
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            requests
        });
        (format!("http://{}/api/generate", address), task)
    }

    fn model_json(name: &str) -> String {
        serde_json::json!({
            "new_name": name,
            "category": "Research",
            "reason": "Matched the organization and document topic."
        })
        .to_string()
    }

    #[test]
    fn scans_only_supported_files_recursively_and_reports_metadata() {
        let folder = unique_temp_dir("scan");
        let nested = folder.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(folder.join("Report.TXT"), "abc").unwrap();
        fs::write(folder.join("ignored.exe"), "no").unwrap();
        fs::write(nested.join("data.JsOn"), "{}").unwrap();

        let files = scan_folder(folder.to_string_lossy().into_owned()).unwrap();
        assert_eq!(files.len(), 2);
        let text_file = files.iter().find(|file| file.name == "Report.TXT").unwrap();
        assert_eq!(text_file.extension, "txt");
        assert_eq!(text_file.size_bytes, 3);
        assert!(text_file.path.ends_with("Report.TXT"));
        assert!(files.iter().any(|file| file.extension == "json" && file.path.contains("nested")));
        assert!(scan_folder(folder.join("Report.TXT").to_string_lossy().into_owned()).is_err());
        assert!(scan_folder(folder.join("missing").to_string_lossy().into_owned()).is_err());
        remove_temp_dir(&folder);
    }

    #[test]
    fn excerpt_strategies_keep_useful_content_and_respect_limits() {
        assert_eq!(smart_excerpt("csv", "a,b\n1,2\n3,4\n5,6"), "a,b\n1,2\n3,4");
        assert_eq!(smart_excerpt("md", "Title\nline two\nline three\n# Important\nlast"), "Title\nline two\nline three\n# Important\nlast");
        assert!(smart_excerpt("json", r#"{"vendor":"Northwind","items":[1,2]}"#).contains("vendor: \"Northwind\""));
        assert_eq!(smart_excerpt("json", "[1,2,3]"), "Root array with 3 items");
        assert_eq!(smart_excerpt("txt", &"x".repeat(1500)).chars().count(), 1400);
    }

    #[tokio::test]
    async fn rejects_unreadable_or_unsupported_documents() {
        let folder = unique_temp_dir("extract-errors");
        let invalid = folder.join("invalid.txt");
        fs::write(&invalid, [0xff, 0xfe]).unwrap();
        assert!(extract_document_snippet(invalid.to_string_lossy().into_owned()).await.is_err());
        assert!(extract_document_snippet(folder.join("missing.txt").to_string_lossy().into_owned()).await.is_err());
        let unsupported = folder.join("image.png");
        fs::write(&unsupported, "not text").unwrap();
        assert!(extract_document_snippet(unsupported.to_string_lossy().into_owned()).await.unwrap_err().contains("Unsupported"));
        let empty = folder.join("empty.md");
        fs::write(&empty, "  ").unwrap();
        assert!(extract_document_snippet(empty.to_string_lossy().into_owned()).await.unwrap_err().contains("No readable text"));
        remove_temp_dir(&folder);
    }

    #[test]
    fn handles_date_boundaries_and_ambiguous_content() {
        let valid_dates = [
            ("Date: 2024-02-29", Some("2024-02-29")),
            ("Date: 2025-02-29", None),
            ("Date: 2026-13-01", None),
            ("Date: 14.09.2026", Some("2026-09-14")),
            ("Date: September 14, 2026", Some("2026-09-14")),
            ("Date: 14. syyskuuta 2026", Some("2026-09-14")),
            ("Date: 2026-09-14 and Date: 2026-09-15", None),
        ];
        for (text, expected) in valid_dates {
            assert_eq!(find_document_date(text, "download.pdf").as_deref(), expected, "{}", text);
        }
    }

    #[test]
    fn parses_model_json_and_rejects_unsafe_filename_candidates() {
        let parsed = parse_model_suggestion(r#"{"new_name":"Northwind_Report.pdf"}"#).unwrap();
        assert_eq!(parsed.category, "");
        assert_eq!(parsed.reason, "");
        assert_eq!(parse_model_suggestion("```json\n{\"new_name\":\"Northwind_Report.pdf\"}\n```").unwrap().new_name, "Northwind_Report.pdf");
        assert!(parse_model_suggestion("{}").is_err());
        assert!(parse_model_suggestion("{\"category\":\"Report\"}").is_err());
        assert_eq!(parse_model_suggestion("Northwind_Report.pdf").unwrap().new_name, "Northwind_Report.pdf");
        assert!(parse_model_suggestion(" ").is_err());

        let suggestion = |name: &str| FilenameSuggestion {
            new_name: name.to_string(),
            category: "Report".to_string(),
            reason: "Source text".to_string(),
        };
        for unsafe_name in ["", "../outside.pdf", "folder\\outside.pdf", ".."] {
            assert!(finalize_suggestion(suggestion(unsafe_name), "pdf", None, None).is_err(), "{}", unsafe_name);
        }
        assert!(finalize_suggestion(suggestion("2026-09-14_Northwind_Report.pdf"), "pdf", None, None).is_err());
        let normalized = finalize_suggestion(suggestion("Northwind.Report.PDF"), "pdf", None, None).unwrap();
        assert_eq!(normalized.new_name, "Northwind_Report.pdf");
        assert_eq!(normalized.category, "Report");
        assert_eq!(normalized.reason, "Source text");
        let retained_extension = finalize_suggestion(suggestion("Northwind_Report.docx"), "pdf", None, None).unwrap();
        assert_eq!(retained_extension.new_name, "Northwind_Report.pdf");
        let reference = finalize_suggestion(suggestion("10024665.pdf"), "pdf", None, None).unwrap();
        assert_eq!(reference.new_name, "10024665.pdf");
    }

    #[test]
    fn loads_defaults_recovers_malformed_config_and_round_trips_settings() {
        let folder = unique_temp_dir("config");
        let path = folder.join("config.json");
        let defaults = load_prompt_config_at(&path);
        assert_eq!(defaults, PromptConfig::default());
        assert!(path.exists());

        fs::write(&path, "not JSON").unwrap();
        assert_eq!(load_prompt_config_at(&path), PromptConfig::default());
        let mut legacy = serde_json::to_value(PromptConfig::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("folder_context_enabled");
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(load_prompt_config_at(&path).folder_context_enabled);

        let saved = PromptConfig {
            system_prompt: "system".into(),
            prompt_template: "{text_snippet}:{extension}:{folder_context}".into(),
            model: "test-model".into(),
            temperature: 0.65,
            folder_context_enabled: false,
        };
        save_prompt_config_at(&path, &saved).unwrap();
        assert_eq!(load_prompt_config_at(&path), saved);
        assert!(save_prompt_config_at(&folder.join("missing/config.json"), &saved).is_err());
        remove_temp_dir(&folder);
    }

    #[tokio::test]
    async fn sends_configured_request_to_local_ollama_and_normalizes_response() {
        let folder = unique_temp_dir("ollama-request");
        let target = folder.join("download.pdf");
        fs::write(&target, "fixture").unwrap();
        let (endpoint, task) = mock_ollama(vec![(200, model_json("Example_Lab_Report.pdf"))]).await;
        let mut config = PromptConfig::default();
        config.system_prompt = "test system prompt".into();
        config.prompt_template = "CTX={folder_context}; EXT={extension}; TEXT={text_snippet}".into();
        config.temperature = 2.5;

        let result = suggest_filename_with_ollama_using_config(
            "Report\nOrganization: Example Lab".into(),
            "pdf".into(),
            Some("selected-model".into()),
            Some("folder overview".into()),
            target.to_string_lossy().into_owned(),
            config,
            &endpoint,
        )
        .await
        .unwrap();
        let requests = task.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "selected-model");
        assert_eq!(requests[0]["stream"], false);
        assert_eq!(requests[0]["options"]["temperature"], 2.0);
        assert_eq!(requests[0]["format"]["required"], serde_json::json!(["new_name", "category", "reason"]));
        assert!(requests[0]["system"].as_str().unwrap().contains("test system prompt"));
        assert!(requests[0]["prompt"].as_str().unwrap().contains("CTX=folder overview; EXT=pdf; TEXT=Report"));
        assert!(!requests[0]["prompt"].as_str().unwrap().contains("Confirmed document date"));
        assert_eq!(result.new_name, "Example_Lab_Report.pdf");
        assert_eq!(result.category, "Research");
        remove_temp_dir(&folder);
    }

    #[tokio::test]
    async fn retries_one_rejected_model_name_and_returns_last_error_after_second_rejection() {
        let folder = unique_temp_dir("ollama-retry");
        let target = folder.join("download.txt");
        fs::write(&target, "fixture").unwrap();
        let (endpoint, task) = mock_ollama(vec![
            (200, model_json("document.txt")),
            (200, model_json("Example_Lab_Report.txt")),
        ]).await;
        let result = suggest_filename_with_ollama_using_config(
            "Report\nOrganization: Example Lab".into(),
            "txt".into(),
            None,
            None,
            target.to_string_lossy().into_owned(),
            PromptConfig::default(),
            &endpoint,
        )
        .await
        .unwrap();
        let requests = task.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]["prompt"].as_str().unwrap().contains("previous filename was rejected"));
        assert_eq!(result.new_name, "Example_Lab_Report.txt");

        let (endpoint, task) = mock_ollama(vec![
            (200, model_json("download.txt")),
            (200, model_json("file.txt")),
        ]).await;
        let error = suggest_filename_with_ollama_using_config(
            "Report\nOrganization: Example Lab".into(),
            "txt".into(),
            None,
            None,
            target.to_string_lossy().into_owned(),
            PromptConfig::default(),
            &endpoint,
        )
        .await
        .unwrap_err();
        assert!(error.contains("could not produce a usable filename"));
        assert_eq!(task.await.unwrap().len(), 2);
        remove_temp_dir(&folder);
    }

    #[tokio::test]
    async fn ollama_connection_failure_returns_a_local_service_error() {
        let folder = unique_temp_dir("ollama-connection-error");
        let target = folder.join("download.txt");
        fs::write(&target, "fixture").unwrap();
        let error = suggest_filename_with_ollama_using_config(
            "Report\nOrganization: Example Lab".into(),
            "txt".into(),
            None,
            None,
            target.to_string_lossy().into_owned(),
            PromptConfig::default(),
            "http://127.0.0.1:0/api/generate",
        )
        .await
        .unwrap_err();
        assert!(error.contains("Failed to connect to Ollama"));
        remove_temp_dir(&folder);
    }

    #[tokio::test]
    async fn ollama_http_error_is_reported_without_retry() {
        let folder = unique_temp_dir("ollama-http-error");
        let target = folder.join("download.txt");
        fs::write(&target, "fixture").unwrap();
        let (endpoint, task) = mock_ollama(vec![(503, "unavailable".into())]).await;
        let error = suggest_filename_with_ollama_using_config(
            "Report\nOrganization: Example Lab".into(),
            "txt".into(),
            None,
            None,
            target.to_string_lossy().into_owned(),
            PromptConfig::default(),
            &endpoint,
        )
        .await
        .unwrap_err();
        assert!(error.contains("503"));
        assert_eq!(task.await.unwrap().len(), 1);
        remove_temp_dir(&folder);
    }

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

    #[test]
    fn recursively_scans_the_large_nested_fixture_batch() {
        let files = scan_folder(fixture("large-batch")).expect("fixture folder should scan");
        assert_eq!(files.len(), 522, "the batch has 504 root files and 18 nested files");
        assert!(files.iter().any(|file| file.name.ends_with(".json")));
        assert!(files.iter().any(|file| file.name.ends_with(".csv")));
        assert!(files.iter().any(|file| file.path.contains("/archive/")));
    }

    #[test]
    fn mixed_folder_context_selects_only_the_same_document_kind_and_subject() {
        let folder = unique_temp_dir("mixed-folder");
        for name in [
            "download (20).pdf",
            "2026-04-10_Nordic_Engineering_Association_Lasku.pdf",
            "2025-11-12_Nordic_Engineering_Association_Invoice.pdf",
            "2024-02-13_Nordic_Engineering_Association_Lasku.pdf",
            "2026-03-12_Nordic_Engineering_Association_Report.pdf",
            "2026-07-01_Alpine_Office_Lasku.pdf",
        ] {
            std::fs::write(folder.join(name), []).expect("fixture file should be created");
        }
        let blueprint = build_directory_blueprint(folder.to_string_lossy().into_owned()).unwrap();
        let siblings = relevant_sibling_names(
            &folder.join("download (20).pdf"),
            "Invoice\nVendor: Nordic Engineering Association\nInvoice date: 2026-09-14",
        );
        let unrelated = relevant_sibling_names(
            &folder.join("download (20).pdf"),
            "Contract\nCustomer: Alpine Office\nSigned: 2026-09-14",
        );
        std::fs::remove_dir_all(&folder).expect("temporary folder should be removed");

        assert!(blueprint.contains("may be unrelated"));
        assert_eq!(siblings.len(), 3);
        assert_eq!(dominant_invoice_label(&siblings), Some("Lasku"));
        assert!(siblings.iter().all(|name| name.contains("Nordic_Engineering_Association")));
        assert!(siblings.iter().all(|name| name.contains("Lasku") || name.contains("Invoice")));
        assert!(unrelated.is_empty());
    }

    #[test]
    fn identifies_issue_date_without_using_invoice_due_date() {
        assert_eq!(
            find_document_date(
                "Laskun päiväys: 14.09.2026\nEräpäivä: 29.09.2026",
                "download (20).pdf"
            ),
            Some("2026-09-14".to_string())
        );
        assert_eq!(
            find_document_date("Date: August 12, 2026", "download_1.txt"),
            Some("2026-08-12".to_string())
        );
        assert_eq!(
            find_document_date("Laskun päiväys: 14. syyskuuta 2026", "download.pdf"),
            Some("2026-09-14".to_string())
        );
        assert_eq!(
            find_document_date("No date in contents", "2026-03-12_invoice.pdf"),
            Some("2026-03-12".to_string())
        );
    }

    #[tokio::test]
    async fn excerpt_preserves_a_date_found_later_in_the_document() {
        let folder = unique_temp_dir("late-date");
        let path = folder.join("download.txt");
        let content = format!(
            "Invoice\nVendor: Nordic Engineering Association\n{}\nInvoice date: 2026-09-14\nClosing note",
            "Item description\n".repeat(10)
        );
        std::fs::write(&path, content).unwrap();
        let snippet = extract_document_snippet(path.to_string_lossy().into_owned()).await.unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        assert!(snippet.starts_with("Document date: 2026-09-14"), "{}", snippet);
    }

    #[test]
    fn moves_confirmed_date_to_front_and_rejects_generic_names() {
        let suggestion = |name: &str| FilenameSuggestion {
            new_name: name.to_string(),
            category: "Invoice".to_string(),
            reason: String::new(),
        };
        let dated = finalize_suggestion(
            suggestion("Tekniikan-Akateemiset-ry-LASKU-2026-09-14.pdf"),
            "pdf",
            Some("2026-09-14"),
            None,
        ).expect("dated invoice should be normalized");
        assert_eq!(
            dated.new_name,
            "2026-09-14_Tekniikan_Akateemiset_Ry_Lasku.pdf"
        );
        let consistent = finalize_suggestion(
            suggestion("Invoice_Nordic_Engineering_Association.pdf"),
            "pdf",
            Some("2026-09-14"),
            Some("Lasku"),
        ).expect("matching invoices should use the same type label");
        assert_eq!(consistent.new_name, "2026-09-14_Nordic_Engineering_Association_Lasku.pdf");
        let finnish = finalize_suggestion(
            suggestion("2025-05-01_Jäsenlasku_TEK.pdf"), "pdf", Some("2025-05-01"), None
        ).unwrap();
        assert_eq!(finnish.new_name, "2025-05-01_Jasenlasku_TEK.pdf");
        assert!(finalize_suggestion(suggestion("download (20).pdf"), "pdf", None, None).is_err());
        assert!(finalize_suggestion(suggestion("2026-10-12-Invoice.pdf"), "pdf", Some("2026-10-12"), None).is_err());
        assert!(finalize_suggestion(suggestion("Nordic_Association_document.pdf"), "pdf", None, None).is_err());
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
    #[ignore = "requires a local Ollama server with qwen2.5-coder:14b"]
    async fn local_ollama_mixed_invoice_smoke() {
        let folder = unique_temp_dir("ollama-smoke");
        for name in [
            "download (20).pdf",
            "2025-11-12_Nordic_Engineering_Association_Lasku.pdf",
            "2024-02-13_Nordic_Engineering_Association_Lasku.pdf",
            "2025-07-14_Nordic_Engineering_Association_Invoice.pdf",
            "2026-03-12_Nordic_Engineering_Association_Report.pdf",
        ] {
            std::fs::write(folder.join(name), []).unwrap();
        }
        let result = suggest_filename_with_ollama(
            "INVOICE\nInvoice date: 2026-09-14\nSupplier: Nordic Engineering Association\nInvoice number: 10024665".to_string(),
            "pdf".to_string(),
            Some("qwen2.5-coder:14b".to_string()),
            Some(build_directory_blueprint(folder.to_string_lossy().into_owned()).unwrap()),
            folder.join("download (20).pdf").to_string_lossy().into_owned(),
        ).await;
        std::fs::remove_dir_all(&folder).unwrap();
        let result = result.expect("local model should suggest a usable filename");
        assert!(result.new_name.starts_with("2026-09-14_"), "{}", result.new_name);
        assert!(result.new_name.contains("Nordic") || result.new_name.contains("10024665"), "{}", result.new_name);
        assert!(!result.new_name.to_lowercase().contains("download"), "{}", result.new_name);
        assert!(result.new_name.to_lowercase().contains("lasku"), "{}", result.new_name);
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
            undo_rename_file,
            get_prompt_config,
            save_prompt_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
