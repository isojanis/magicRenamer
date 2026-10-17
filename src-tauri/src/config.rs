use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub prompt_template: String,
    pub model: String,
    pub temperature: f32,
    #[serde(default = "default_folder_context_enabled")]
    pub folder_context_enabled: bool,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default)]
    pub gemini_api_key: String,
    #[serde(default)]
    pub anthropic_api_key: String,
    #[serde(default)]
    pub openrouter_api_key: String,
    #[serde(default)]
    pub custom_base_url: String,
}

fn default_folder_context_enabled() -> bool { true }
fn default_provider() -> String { "ollama".to_string() }

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prompt: "Use the document and directory context to suggest a concise filename that follows the directory's conventions. Return only valid JSON with string fields new_name, category, and reason. new_name must be a filename, not a path, and retain the target extension.".to_string(),
            prompt_template: "Directory context:\n{folder_context}\n\nTarget extension: .{extension}\nDocument excerpt:\n\"\"\"\n{text_snippet}\n\"\"\"\nReturn JSON with new_name, category, and reason.".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            temperature: 0.2,
            folder_context_enabled: true,
            provider: "ollama".to_string(),
            openai_api_key: String::new(),
            gemini_api_key: String::new(),
            anthropic_api_key: String::new(),
            openrouter_api_key: String::new(),
            custom_base_url: String::new(),
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

pub(crate) fn load_prompt_config() -> PromptConfig {
    load_prompt_config_at(&get_config_path())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn get_prompt_config() -> Result<PromptConfig, String> {
    Ok(load_prompt_config())
}

fn save_prompt_config_at(path: &Path, config: &PromptConfig) -> Result<(), String> {
    let data = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(path, data).map_err(|e| format!("Failed to write config file to {:?}: {}", path, e))?;
    Ok(())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn save_prompt_config(config: PromptConfig) -> Result<(), String> {
    save_prompt_config_at(&get_config_path(), &config)
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
