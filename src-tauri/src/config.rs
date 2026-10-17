use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

fn get_config_path() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "Could not locate the user configuration directory.".to_string())?;
    let folder = base.join("MagicRenamer");
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the application settings directory: {}", error))?;
    let metadata = fs::symlink_metadata(&folder)
        .map_err(|error| format!("Could not inspect the application settings directory: {}", error))?;
    if !metadata.file_type().is_dir() {
        return Err("The application settings directory must not be a symlink.".into());
    }
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not secure the application settings directory: {}", error))?;
    }
    Ok(folder.join("config.json"))
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

fn read_optional_config_file(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(format!("Settings path is not a regular file: {}", path.display()))
        }
        Ok(_) => fs::read_to_string(path).map(Some)
            .map_err(|error| format!("Could not read settings file: {}", error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not inspect settings file: {}", error)),
    }
}

fn load_prompt_config_at(path: &Path) -> Result<PromptConfig, String> {
    if let Some(content) = read_optional_config_file(path)? {
        if let Ok(mut cfg) = serde_json::from_str::<PromptConfig>(&content) {
            let key_path = api_keys_path(path);
            let had_inline_keys = !StoredApiKeys::from_config(&cfg).is_empty();
            if let Some(keys_json) = read_optional_config_file(&key_path)? {
                let keys: StoredApiKeys = serde_json::from_str(&keys_json)
                    .map_err(|_| "Saved API key file is not valid JSON.".to_string())?;
                keys.apply_to(&mut cfg);
                if had_inline_keys { write_public_config_at(path, &cfg)?; }
            } else if had_inline_keys {
                save_prompt_config_at(path, &cfg)?;
            }
            return Ok(cfg);
        }
    }
    let mut default_cfg = PromptConfig::default();
    if let Some(keys_json) = read_optional_config_file(&api_keys_path(path))? {
        let keys: StoredApiKeys = serde_json::from_str(&keys_json)
            .map_err(|_| "Saved API key file is not valid JSON.".to_string())?;
        keys.apply_to(&mut default_cfg);
    }
    write_public_config_at(path, &default_cfg)?;
    Ok(default_cfg)
}

pub(crate) fn load_prompt_config() -> Result<PromptConfig, String> {
    load_prompt_config_at(&get_config_path()?)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn get_prompt_config() -> Result<PublicPromptConfig, String> {
    Ok(PublicPromptConfig::from(&load_prompt_config()?))
}

static SETTINGS_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_private_file(path: &Path, data: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(format!("Settings path is not a regular file: {}", path.display()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not inspect settings path: {}", error)),
    }
    let parent = path.parent().ok_or_else(|| "Settings path has no parent directory.".to_string())?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("Could not inspect settings directory: {}", error))?;
    if !parent_metadata.file_type().is_dir() {
        return Err("Settings directory must not be a symlink.".into());
    }
    let nonce = SETTINGS_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".magicrenamer-settings-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|error| format!("Could not create temporary settings file: {}", error))?;
        use std::io::Write;
        file.write_all(data.as_bytes()).map_err(|error| format!("Could not write settings file: {}", error))?;
        file.sync_all().map_err(|error| format!("Could not sync settings file: {}", error))?;
        drop(file);
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(format!("Settings path is not a regular file: {}", path.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not inspect settings path: {}", error)),
        }
        fs::rename(&temp, path).map_err(|error| format!("Could not replace settings file: {}", error))
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
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
    let merged = apply_config_update(load_prompt_config()?, config)?;
    save_prompt_config_at(&get_config_path()?, &merged)?;
    Ok(PublicPromptConfig::from(&merged))
}

fn import_prompt_config_at(source: &Path, destination: &Path) -> Result<PromptConfig, String> {
    if source == destination { return Err("Choose a previous settings file to import.".into()); }
    if source.file_name().and_then(|name| name.to_str()) != Some("config.json") {
        return Err("Choose the previous config.json file.".into());
    }
    let content = read_optional_config_file(source)?
        .ok_or_else(|| "Previous settings file does not exist.".to_string())?;
    let mut config: PromptConfig = serde_json::from_str(&content)
        .map_err(|_| "Previous settings file is not valid JSON.".to_string())?;
    if !valid_provider(&config.provider) || config.model.trim().is_empty()
        || !config.temperature.is_finite() || !(0.0..=2.0).contains(&config.temperature)
    {
        return Err("Previous settings contain an invalid provider, model, or temperature.".into());
    }
    if !config.custom_base_url.is_empty() { crate::ai::validate_endpoint(&config.custom_base_url)?; }
    if let Some(keys_json) = read_optional_config_file(&api_keys_path(source))? {
        let keys: StoredApiKeys = serde_json::from_str(&keys_json)
            .map_err(|_| "Previous API key file is not valid JSON.".to_string())?;
        keys.apply_to(&mut config);
    }
    save_prompt_config_at(destination, &config)?;
    Ok(config)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) fn import_prompt_config(file_path: String) -> Result<PublicPromptConfig, String> {
    let source = Path::new(&file_path);
    let imported = import_prompt_config_at(source, &get_config_path()?)?;
    Ok(PublicPromptConfig::from(&imported))
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
