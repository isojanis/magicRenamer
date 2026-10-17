use crate::config::{load_prompt_config, PromptConfig};
use crate::naming::{
    document_kind, dominant_invoice_label, finalize_suggestion, find_document_date,
    parse_model_suggestion, relevant_sibling_names, FilenameSuggestion,
    FOLDER_CONTEXT_INSTRUCTIONS, UNIVERSAL_NAMING_INSTRUCTIONS,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

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

/// Suggests a filename for one document using its content and only matching sibling names.
#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) async fn suggest_filename_with_ollama(
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
    let provider = config.provider.to_lowercase();
    match provider.as_str() {
        "openai" | "openrouter" => {
            let (default_url, api_key, auth_header) = if provider == "openrouter" {
                (
                    "https://openrouter.ai/api/v1/chat/completions",
                    if !config.openrouter_api_key.is_empty() {
                        config.openrouter_api_key.clone()
                    } else {
                        std::env::var("OPENROUTER_API_KEY").unwrap_or_default()
                    },
                    "Bearer",
                )
            } else {
                (
                    "https://api.openai.com/v1/chat/completions",
                    if !config.openai_api_key.is_empty() {
                        config.openai_api_key.clone()
                    } else {
                        std::env::var("OPENAI_API_KEY").unwrap_or_default()
                    },
                    "Bearer",
                )
            };

            let endpoint = if let Some(url) = custom_endpoint {
                url
            } else if !config.custom_base_url.is_empty() {
                &config.custom_base_url
            } else {
                default_url
            };

            if api_key.is_empty() && !endpoint.contains("localhost") && !endpoint.contains("127.0.0.1") {
                return Err(format!("No API key configured for {}. Set it in Prompt Settings or environment variable.", provider));
            }

            let payload = serde_json::json!({
                "model": model,
                "temperature": temperature,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": prompt }
                ],
                "response_format": { "type": "json_object" }
            });

            let mut req = client.post(endpoint).json(&payload);
            if !api_key.is_empty() {
                req = req.header("Authorization", format!("{} {}", auth_header, api_key));
            }

            let response = req.send().await
                .map_err(|error| format!("Failed to connect to {} ({}): {}", provider, endpoint, error))?;

            if !response.status().is_success() {
                let status = response.status();
                let err_text = response.text().await.unwrap_or_default();
                return Err(format!("{} API returned error {}: {}", provider, status, err_text));
            }

            let res_json: serde_json::Value = response.json().await
                .map_err(|error| format!("Failed to parse response from {}: {}", provider, error))?;

            let content = res_json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| format!("Invalid response format from {}", provider))?;
            Ok(content.to_string())
        }
        "gemini" => {
            let api_key = if !config.gemini_api_key.is_empty() {
                config.gemini_api_key.clone()
            } else {
                std::env::var("GEMINI_API_KEY").unwrap_or_default()
            };

            if api_key.is_empty() {
                return Err("No Gemini API key configured. Set it in Prompt Settings or GEMINI_API_KEY env var.".into());
            }

            let endpoint = if let Some(url) = custom_endpoint {
                url.to_string()
            } else {
                format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                    model, api_key
                )
            };

            let payload = serde_json::json!({
                "systemInstruction": {
                    "parts": [{ "text": system_prompt }]
                },
                "contents": [
                    { "parts": [{ "text": prompt }] }
                ],
                "generationConfig": {
                    "temperature": temperature,
                    "responseMimeType": "application/json"
                }
            });

            let response = client.post(&endpoint)
                .json(&payload)
                .send()
                .await
                .map_err(|error| format!("Failed to connect to Gemini API: {}", error))?;

            if !response.status().is_success() {
                let status = response.status();
                let err_text = response.text().await.unwrap_or_default();
                return Err(format!("Gemini API returned error {}: {}", status, err_text));
            }

            let res_json: serde_json::Value = response.json().await
                .map_err(|error| format!("Failed to parse Gemini response: {}", error))?;

            let content = res_json["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .ok_or_else(|| "Invalid response format from Gemini".to_string())?;
            Ok(content.to_string())
        }
        "anthropic" => {
            let api_key = if !config.anthropic_api_key.is_empty() {
                config.anthropic_api_key.clone()
            } else {
                std::env::var("ANTHROPIC_API_KEY").unwrap_or_default()
            };

            if api_key.is_empty() {
                return Err("No Anthropic API key configured. Set it in Prompt Settings or ANTHROPIC_API_KEY env var.".into());
            }

            let endpoint = custom_endpoint.unwrap_or("https://api.anthropic.com/v1/messages");

            let payload = serde_json::json!({
                "model": model,
                "system": system_prompt,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "temperature": temperature,
                "max_tokens": 1024
            });

            let response = client.post(endpoint)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&payload)
                .send()
                .await
                .map_err(|error| format!("Failed to connect to Anthropic API: {}", error))?;

            if !response.status().is_success() {
                let status = response.status();
                let err_text = response.text().await.unwrap_or_default();
                return Err(format!("Anthropic API returned error {}: {}", status, err_text));
            }

            let res_json: serde_json::Value = response.json().await
                .map_err(|error| format!("Failed to parse Anthropic response: {}", error))?;

            let content = res_json["content"][0]["text"]
                .as_str()
                .ok_or_else(|| "Invalid response format from Anthropic".to_string())?;
            Ok(content.to_string())
        }
        _ => {
            // Default: local Ollama
            let endpoint = if let Some(url) = custom_endpoint {
                url
            } else if !config.custom_base_url.is_empty() {
                &config.custom_base_url
            } else {
                "http://localhost:11434/api/generate"
            };

            let payload = OllamaGenerateRequest {
                model: model.to_string(),
                prompt: prompt.to_string(),
                system: system_prompt.to_string(),
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

            Ok(generated.response)
        }
    }
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
