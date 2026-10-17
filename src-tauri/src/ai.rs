use crate::config::{apply_config_update, load_prompt_config, PromptConfig, PromptConfigUpdate};
use crate::naming::{
    document_kind, dominant_invoice_label, finalize_suggestion, find_document_date,
    parse_model_suggestion, relevant_sibling_names, FilenameSuggestion,
    FOLDER_CONTEXT_INSTRUCTIONS, UNIVERSAL_NAMING_INSTRUCTIONS,
};
use serde::Serialize;
use std::future::Future;
use std::path::Path;
use std::sync::OnceLock;
use tokio::sync::watch;

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

const PROCESSING_STOPPED: &str = "Processing stopped.";

fn processing_runs() -> &'static watch::Sender<u64> {
    static RUNS: OnceLock<watch::Sender<u64>> = OnceLock::new();
    RUNS.get_or_init(|| {
        let (sender, _) = watch::channel(0);
        sender
    })
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn begin_processing_run() -> u64 {
    let mut run_id = 0;
    processing_runs().send_modify(|current| {
        *current = current.wrapping_add(1).max(1);
        run_id = *current;
    });
    run_id
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn cancel_processing_run(processing_run_id: u64) -> bool {
    processing_runs().send_if_modified(|current| {
        if *current != processing_run_id { return false; }
        *current = current.wrapping_add(1).max(1);
        true
    })
}

async fn run_for_processing_run<T>(processing_run_id: u64, work: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    let mut receiver = processing_runs().subscribe();
    let cancelled = async move {
        loop {
            if *receiver.borrow() != processing_run_id { return; }
            if receiver.changed().await.is_err() { return; }
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Err(PROCESSING_STOPPED.into()),
        result = work => result,
    }
}

/// Suggests a filename for one document using its content and only matching sibling names.
#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) async fn suggest_filename_with_ollama(
    text_snippet: String,
    extension: String,
    model: Option<String>,
    folder_context: Option<String>,
    file_path: String,
    processing_run_id: Option<u64>,
) -> Result<FilenameSuggestion, String> {
    let work = suggest_filename_with_ollama_using_config(
        text_snippet,
        extension,
        model,
        folder_context,
        file_path,
        load_prompt_config(),
        "http://localhost:11434/api/generate",
    );
    match processing_run_id {
        Some(run_id) => run_for_processing_run(run_id, work).await,
        None => work.await,
    }
}

pub(crate) fn validate_endpoint(endpoint: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "Enter a valid endpoint URL.".to_string())?;
    let host = url.host_str().unwrap_or_default();
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Use HTTPS for remote endpoints, or HTTP on localhost.".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err("Endpoint URLs cannot contain credentials, query parameters, or fragments.".into());
    }
    Ok(url)
}

fn provider_key(config: &PromptConfig) -> String {
    let (saved, env_name) = match config.provider.as_str() {
        "openai" => (&config.openai_api_key, "OPENAI_API_KEY"),
        "gemini" => (&config.gemini_api_key, "GEMINI_API_KEY"),
        "anthropic" => (&config.anthropic_api_key, "ANTHROPIC_API_KEY"),
        "openrouter" => (&config.openrouter_api_key, "OPENROUTER_API_KEY"),
        _ => return String::new(),
    };
    if saved.is_empty() { std::env::var(env_name).unwrap_or_default() } else { saved.clone() }
}

fn provider_endpoint(config: &PromptConfig, model: &str, override_endpoint: Option<&str>) -> Result<reqwest::Url, String> {
    let endpoint = if let Some(endpoint) = override_endpoint {
        endpoint.to_string()
    } else if !config.custom_base_url.is_empty() && matches!(config.provider.as_str(), "ollama" | "openai" | "openrouter") {
        config.custom_base_url.clone()
    } else {
        match config.provider.as_str() {
            "ollama" => "http://localhost:11434/api/generate".into(),
            "openai" => "https://api.openai.com/v1/chat/completions".into(),
            "openrouter" => "https://openrouter.ai/api/v1/chat/completions".into(),
            "anthropic" => "https://api.anthropic.com/v1/messages".into(),
            "gemini" => {
                if !model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
                    return Err("Gemini model contains unsupported characters.".into());
                }
                format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", model)
            }
            _ => return Err("Choose a supported AI provider.".into()),
        }
    };
    validate_endpoint(&endpoint)
}

async fn query_ai_provider(
    client: &reqwest::Client,
    config: &PromptConfig,
    model: &str,
    system_prompt: &str,
    prompt: &str,
    temperature: f32,
    schema: &serde_json::Value,
    custom_endpoint: Option<&str>,
) -> Result<String, String> {
    let provider = config.provider.as_str();
    let endpoint = provider_endpoint(config, model, custom_endpoint)?;
    let key = provider_key(config);
    if provider != "ollama" && key.is_empty() && !endpoint.host_str().is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "[::1]")) {
        return Err(format!("No {} API key configured. Add it in Settings or set the provider environment variable.", provider));
    }
    let request = match provider {
        "openai" | "openrouter" => {
            let mut payload = serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": prompt }
                ],
                "response_format": { "type": "json_object" }
            });
            if !(provider == "openai" && (model.starts_with("gpt-5") || model.starts_with("gpt-6") || model.starts_with("o3") || model.starts_with("o4")))
                && !(provider == "openrouter" && model == "openrouter/auto") {
                payload["temperature"] = serde_json::json!(temperature);
            }
            let mut request = client.post(endpoint).json(&payload);
            if !key.is_empty() { request = request.bearer_auth(&key); }
            request
        }
        "gemini" => {
            let payload = serde_json::json!({
                "systemInstruction": { "parts": [{ "text": system_prompt }] },
                "contents": [{ "parts": [{ "text": prompt }] }],
                "generationConfig": { "temperature": temperature, "responseMimeType": "application/json" }
            });
            client.post(endpoint).header("x-goog-api-key", &key).json(&payload)
        }
        "anthropic" => {
            let mut payload = serde_json::json!({
                "model": model, "system": system_prompt,
                "messages": [{ "role": "user", "content": prompt }],
                "max_tokens": 2048
            });
            if !model.starts_with("claude-haiku-5") && !model.starts_with("claude-sonnet-5")
                && !model.starts_with("claude-opus-5") {
                payload["temperature"] = serde_json::json!(temperature);
            }
            client.post(endpoint).header("x-api-key", &key).header("anthropic-version", "2023-06-01").json(&payload)
        }
        "ollama" => {
            let payload = OllamaGenerateRequest {
                model: model.to_string(), prompt: prompt.to_string(), system: system_prompt.to_string(),
                stream: false, format: schema.clone(), options: OllamaOptions { temperature },
            };
            client.post(endpoint).json(&payload)
        }
        _ => return Err("Choose a supported AI provider.".into()),
    };
    // The client disables redirects so credentials and document text cannot be forwarded to another host.
    let response = request.send().await.map_err(|_| {
        if provider == "ollama" {
            "Failed to connect to Ollama. Start Ollama, or choose a cloud provider and add its API key in Settings.".to_string()
        } else {
            format!("Failed to connect to {}. Check its endpoint and network connection.", provider)
        }
    })?;
    if !response.status().is_success() {
        let status = response.status();
        let guidance = match status.as_u16() {
            401 | 403 => " Check the API key and account access.",
            404 => " Check the model and endpoint URL.",
            429 => " The provider rate limit was reached.",
            _ => "",
        };
        return Err(format!("{} API returned HTTP {}.{}", provider, status, guidance));
    }
    let json: serde_json::Value = response.json().await
        .map_err(|_| format!("Failed to parse {} response.", provider))?;
    let content = match provider {
        "openai" | "openrouter" => json["choices"][0]["message"]["content"].as_str().map(str::to_string),
        "gemini" => json["candidates"][0]["content"]["parts"].as_array().map(|parts| {
            parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("")
        }),
        "anthropic" => json["content"].as_array().map(|parts| {
            parts.iter().filter(|part| part["type"] == "text")
                .filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("")
        }),
        "ollama" => json["response"].as_str().map(str::to_string),
        _ => None,
    };
    content.filter(|text| !text.is_empty()).ok_or_else(|| format!("Invalid response format from {}.", provider))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) async fn test_ai_provider(config: PromptConfigUpdate) -> Result<String, String> {
    let config = apply_config_update(load_prompt_config(), config)?;
    test_ai_provider_with_config(&config, None).await
}

async fn test_ai_provider_with_config(config: &PromptConfig, endpoint: Option<&str>) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|_| "Could not create HTTP client.".to_string())?;
    let schema = serde_json::json!({
        "type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"]
    });
    query_ai_provider(&client, config, &config.model, "Respond only with JSON.", "Return {\"ok\":true}.",
        config.temperature, &schema, endpoint).await?;
    Ok(format!("Connected to {} using {}.", config.provider, config.model))
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|_| "Could not create HTTP client.".to_string())?;
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
        let response_text = query_ai_provider(
            &client,
            &config,
            &selected_model,
            &system_prompt,
            &prompt,
            temperature,
            &schema,
            if endpoint.is_empty() || endpoint == "http://localhost:11434/api/generate" { None } else { Some(endpoint) },
        ).await?;

        let result = parse_model_suggestion(&response_text)
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
    Err(format!("AI model could not produce a usable filename: {}", last_issue))
}

#[cfg(test)]
#[path = "tests/ai.rs"]
mod tests;
