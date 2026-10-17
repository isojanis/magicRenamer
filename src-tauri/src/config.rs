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

pub(crate) fn valid_provider(provider: &str) -> bool {
    matches!(provider, "ollama" | "openai" | "gemini" | "anthropic" | "openrouter")
}

#[derive(Serialize)]
pub(crate) struct PublicPromptConfig {
    pub system_prompt: String,
    pub prompt_template: String,
    pub model: String,
    pub temperature: f32,
    pub folder_context_enabled: bool,
    pub provider: String,
    pub custom_base_url: String,
    pub configured_api_keys: ConfiguredApiKeys,
}

#[derive(Serialize)]
pub(crate) struct ConfiguredApiKeys {
    pub openai: bool,
    pub gemini: bool,
    pub anthropic: bool,
    pub openrouter: bool,
}

impl From<&PromptConfig> for PublicPromptConfig {
    fn from(config: &PromptConfig) -> Self {
        Self {
            system_prompt: config.system_prompt.clone(),
            prompt_template: config.prompt_template.clone(),
            model: config.model.clone(),
            temperature: config.temperature,
            folder_context_enabled: config.folder_context_enabled,
            provider: config.provider.clone(),
            custom_base_url: config.custom_base_url.clone(),
            configured_api_keys: ConfiguredApiKeys {
                openai: !config.openai_api_key.is_empty() || std::env::var("OPENAI_API_KEY").is_ok_and(|key| !key.is_empty()),
                gemini: !config.gemini_api_key.is_empty() || std::env::var("GEMINI_API_KEY").is_ok_and(|key| !key.is_empty()),
                anthropic: !config.anthropic_api_key.is_empty() || std::env::var("ANTHROPIC_API_KEY").is_ok_and(|key| !key.is_empty()),
                openrouter: !config.openrouter_api_key.is_empty() || std::env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.is_empty()),
            },
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct PromptConfigUpdate {
    pub system_prompt: String,
    pub prompt_template: String,
    pub model: String,
    pub temperature: f32,
    pub folder_context_enabled: bool,
    pub provider: String,
    #[serde(default)]
    pub custom_base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub clear_api_key: bool,
}

pub(crate) fn apply_config_update(mut config: PromptConfig, update: PromptConfigUpdate) -> Result<PromptConfig, String> {
    let provider = update.provider.trim().to_lowercase();
    if !valid_provider(&provider) {
        return Err("Choose a supported AI provider.".into());
    }
    if update.model.trim().is_empty() {
        return Err("Choose a model for the selected provider.".into());
    }
    if !update.temperature.is_finite() || !(0.0..=2.0).contains(&update.temperature) {
        return Err("Temperature must be between 0 and 2.".into());
    }
    if !update.custom_base_url.trim().is_empty() {
        crate::ai::validate_endpoint(&update.custom_base_url)?;
    }
    config.system_prompt = update.system_prompt;
    config.prompt_template = update.prompt_template;
    config.model = update.model.trim().to_string();
    config.temperature = update.temperature;
    config.folder_context_enabled = update.folder_context_enabled;
    config.provider = provider.clone();
    config.custom_base_url = update.custom_base_url.trim().to_string();
    let key = match provider.as_str() {
        "openai" => Some(&mut config.openai_api_key),
        "gemini" => Some(&mut config.gemini_api_key),
        "anthropic" => Some(&mut config.anthropic_api_key),
        "openrouter" => Some(&mut config.openrouter_api_key),
        _ => None,
    };
    if let Some(key) = key {
        if update.clear_api_key { key.clear(); }
        else if !update.api_key.trim().is_empty() { *key = update.api_key.trim().to_string(); }
    }
    Ok(config)
}

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

#[derive(Serialize, Deserialize, Default)]
struct StoredApiKeys {
    #[serde(default)] openai_api_key: String,
    #[serde(default)] gemini_api_key: String,
    #[serde(default)] anthropic_api_key: String,
    #[serde(default)] openrouter_api_key: String,
}

impl StoredApiKeys {
    fn from_config(config: &PromptConfig) -> Self {
        Self {
            openai_api_key: config.openai_api_key.clone(),
            gemini_api_key: config.gemini_api_key.clone(),
            anthropic_api_key: config.anthropic_api_key.clone(),
            openrouter_api_key: config.openrouter_api_key.clone(),
        }
    }
    fn is_empty(&self) -> bool {
        self.openai_api_key.is_empty() && self.gemini_api_key.is_empty()
            && self.anthropic_api_key.is_empty() && self.openrouter_api_key.is_empty()
    }
    fn apply_to(self, config: &mut PromptConfig) {
        config.openai_api_key = self.openai_api_key;
        config.gemini_api_key = self.gemini_api_key;
        config.anthropic_api_key = self.anthropic_api_key;
        config.openrouter_api_key = self.openrouter_api_key;
    }
}

fn api_keys_path(path: &Path) -> std::path::PathBuf {
    path.with_file_name("config.local.json")
}

fn load_prompt_config_at(path: &Path) -> PromptConfig {
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(mut cfg) = serde_json::from_str::<PromptConfig>(&content) {
            let key_path = api_keys_path(path);
            let had_inline_keys = !StoredApiKeys::from_config(&cfg).is_empty();
            if let Ok(keys_json) = fs::read_to_string(&key_path) {
                if let Ok(keys) = serde_json::from_str::<StoredApiKeys>(&keys_json) {
                    keys.apply_to(&mut cfg);
                    if had_inline_keys { let _ = write_public_config_at(path, &cfg); }
                }
            } else if had_inline_keys {
                // Migrate older inline credentials to the ignored local file.
                let _ = save_prompt_config_at(path, &cfg);
            }
            return cfg;
        }
    }
    let mut default_cfg = PromptConfig::default();
    if let Ok(keys_json) = fs::read_to_string(api_keys_path(path)) {
        if let Ok(keys) = serde_json::from_str::<StoredApiKeys>(&keys_json) {
            keys.apply_to(&mut default_cfg);
        }
    }
    let _ = write_public_config_at(path, &default_cfg);
    default_cfg
}

pub(crate) fn load_prompt_config() -> PromptConfig {
    load_prompt_config_at(&get_config_path())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn get_prompt_config() -> Result<PublicPromptConfig, String> {
    Ok(PublicPromptConfig::from(&load_prompt_config()))
}

fn write_private_file(path: &Path, data: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600)
            .open(path).map_err(|e| format!("Failed to open config file: {}", e))?;
        file.write_all(data.as_bytes()).map_err(|e| format!("Failed to write config file: {}", e))?;
        let mut permissions = file.metadata().map_err(|e| e.to_string())?.permissions();
        permissions.set_mode(0o600);
        file.set_permissions(permissions).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    fs::write(path, data).map_err(|e| format!("Failed to write config file: {}", e))?;
    Ok(())
}

fn write_public_config_at(path: &Path, config: &PromptConfig) -> Result<(), String> {
    let mut public = config.clone();
    StoredApiKeys::default().apply_to(&mut public);
    let data = serde_json::to_string_pretty(&public)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    write_private_file(path, &data)
}

fn save_prompt_config_at(path: &Path, config: &PromptConfig) -> Result<(), String> {
    let keys = StoredApiKeys::from_config(config);
    let key_path = api_keys_path(path);
    if !keys.is_empty() {
        let data = serde_json::to_string_pretty(&keys)
            .map_err(|e| format!("Failed to serialize API keys: {}", e))?;
        write_private_file(&key_path, &data)?;
    }
    write_public_config_at(path, config)?;
    if keys.is_empty() && key_path.exists() {
        fs::remove_file(&key_path).map_err(|e| format!("Failed to remove local API keys: {}", e))?;
    }
    Ok(())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn save_prompt_config(config: PromptConfigUpdate) -> Result<PublicPromptConfig, String> {
    let merged = apply_config_update(load_prompt_config(), config)?;
    save_prompt_config_at(&get_config_path(), &merged)?;
    Ok(PublicPromptConfig::from(&merged))
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
