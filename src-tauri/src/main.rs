use chrono::Utc;
use office_oxide::Document;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
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
    let client = reqwest::Client::new();
    let config = load_prompt_config();
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
        let response = client.post("http://localhost:11434/api/generate")
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

fn update_or_create_manifest(dir: &Path, mapping: RenameMapping) -> Result<(), String> {
    // Format: rename_history_YYYYMMDD_HHMM.json
    let now = Utc::now();
    let prefix = format!("rename_history_{}", now.format("%Y%m%d_%H%M"));

    // Find if a manifest for the current minute already exists in dir, or create a new one
    let mut target_manifest_path = dir.join(format!("{}.json", prefix));

    if !target_manifest_path.exists() {
        // Also check if any manifest was created in the last 15 minutes to group session renames
        if let Ok(entries) = fs::read_dir(dir) {
            let mut recent_manifest: Option<std::path::PathBuf> = None;
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("rename_history_") && name.ends_with(".json") {
                    recent_manifest = Some(entry.path());
                }
            }
            if let Some(m) = recent_manifest {
                target_manifest_path = m;
            }
        }
    }

    let mut manifest = if target_manifest_path.exists() {
        let content = fs::read_to_string(&target_manifest_path).unwrap_or_default();
        serde_json::from_str::<RenameHistoryManifest>(&content).unwrap_or_else(|_| RenameHistoryManifest {
            version: 1,
            session_timestamp: now.to_rfc3339(),
            mappings: Vec::new(),
        })
    } else {
        RenameHistoryManifest {
            version: 1,
            session_timestamp: now.to_rfc3339(),
            mappings: Vec::new(),
        }
    };

    // Update existing mapping if current_name matches or append new
    if let Some(existing) = manifest.mappings.iter_mut().find(|m| m.current_name == mapping.current_name) {
        *existing = mapping;
    } else {
        manifest.mappings.push(mapping);
    }

    let json_bytes = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(&target_manifest_path, json_bytes).map_err(|e| format!("Failed to write manifest: {}", e))?;
    Ok(())
}

/// 3. Safely renames the file on the local filesystem with collision handling,
/// xattr tagging, and manifest recording.
#[cfg_attr(feature = "desktop", tauri::command)]
async fn rename_file(old_path: String, new_path: String) -> Result<String, String> {
    let source = Path::new(&old_path);
    if !source.exists() {
        return Err(format!("Source file does not exist: {}", old_path));
    }

    let original_file_name = source
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let sha256_hash = compute_file_sha256(source).unwrap_or_default();

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

    // 1. Tag the file with xattr (zero-mutation local metadata)
    set_xattr_original_name(&final_dest, &original_file_name);

    // 2. Append to visible session manifest rename_history_YYYYMMDD_HHMM.json
    if let Some(parent) = final_dest.parent() {
        let current_file_name = final_dest
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let mapping = RenameMapping {
            current_name: current_file_name,
            original_name: original_file_name,
            sha256: sha256_hash,
            renamed_at: Utc::now().to_rfc3339(),
        };

        let _ = update_or_create_manifest(parent, mapping);
    }

    Ok(final_dest.to_string_lossy().to_string())
}

/// Undoes a rename by restoring the original filename from xattr or local directory manifests.
#[cfg_attr(feature = "desktop", tauri::command)]
async fn undo_rename_file(file_path: String) -> Result<UndoResult, String> {
    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(format!("File does not exist: {}", file_path));
    }

    let parent = path.parent().ok_or_else(|| "File has no parent directory".to_string())?;
    let current_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();

    // Priority 1: Check xattr metadata
    if let Some(orig_name) = get_xattr_original_name(path) {
        if !orig_name.is_empty() && orig_name != current_name {
            let target = parent.join(&orig_name);
            fs::rename(path, &target).map_err(|e| format!("Failed to revert filename: {}", e))?;
            remove_xattr_original_name(&target);
            return Ok(UndoResult {
                restored_path: target.to_string_lossy().to_string(),
                original_name: orig_name,
                source: "xattr".to_string(),
            });
        }
    }

    // Priority 2: Check any rename_history_*.json in the parent folder
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.filter_map(|e| e.ok()) {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with("rename_history_") && fname.ends_with(".json") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    if let Ok(manifest) = serde_json::from_str::<RenameHistoryManifest>(&content) {
                        if let Some(mapping) = manifest.mappings.iter().find(|m| m.current_name == current_name) {
                            let orig_name = mapping.original_name.clone();
                            let target = parent.join(&orig_name);
                            fs::rename(path, &target).map_err(|e| format!("Failed to revert filename: {}", e))?;
                            remove_xattr_original_name(&target);
                            return Ok(UndoResult {
                                restored_path: target.to_string_lossy().to_string(),
                                original_name: orig_name,
                                source: "manifest".to_string(),
                            });
                        }
                    }
                }
            }
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
        let folder = std::env::temp_dir().join(format!(
            "magicrenamer-mixed-folder-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&folder).expect("temporary folder should be created");
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
        let folder = std::env::temp_dir().join(format!(
            "magicrenamer-late-date-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&folder).unwrap();
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
        let folder = std::env::temp_dir().join(format!(
            "magicrenamer-ollama-smoke-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&folder).unwrap();
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
        let temp_dir = std::env::temp_dir().join(format!("magicrenamer-undo-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

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
