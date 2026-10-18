use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FilenameSuggestion {
    pub new_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub reason: String,
}

/// Summarizes the selected folder without assuming its files share a document type.
pub(crate) fn build_directory_blueprint(folder_path: String) -> Result<String, String> {
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

pub(crate) fn document_kind(text: &str) -> Option<&'static str> {
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

pub(crate) fn relevant_sibling_names(file_path: &Path, snippet: &str) -> Vec<String> {
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

pub(crate) fn dominant_invoice_label(names: &[String]) -> Option<&'static str> {
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

pub(crate) const UNIVERSAL_NAMING_INSTRUCTIONS: &str = "Name the target document from its own contents. If a reliable full document date is provided, new_name must start with YYYY-MM-DD_. Put the subject or organization before the document type when both are known. Use the full organization name from the target when available. Use underscores between fields. Do not invent a date, organization, or document type. Never return a generic name such as download, scan, file, document, invoice, or lasku alone. These requirements and the JSON response format override conflicting instructions earlier in this system prompt. Return only a JSON object with string fields new_name, category, and reason. Keep the original file extension.";
pub(crate) const FOLDER_CONTEXT_INSTRUCTIONS: &str = "A folder can contain unrelated documents. Use only sibling names explicitly identified as relevant to this target. Matching siblings can guide spelling and category wording for the same kind of document; never copy their dates or subjects into unrelated files. The target document determines its date, subject, and type.";

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

pub(crate) fn find_document_date(text: &str, original_name: &str) -> Option<String> {
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

pub(crate) fn finalize_suggestion(
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

pub(crate) fn parse_model_suggestion(response: &str) -> Result<FilenameSuggestion, String> {
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

#[cfg(test)]
#[path = "tests/naming.rs"]
mod tests;
