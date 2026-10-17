use super::*;
use crate::test_support::*;

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
        ..Default::default()
    };
    save_prompt_config_at(&path, &saved).unwrap();
    assert_eq!(load_prompt_config_at(&path), saved);
    assert!(save_prompt_config_at(&folder.join("missing/config.json"), &saved).is_err());
    remove_temp_dir(&folder);
}

#[test]
fn updating_one_provider_preserves_other_keys_and_redacts_public_config() {
    let existing = PromptConfig {
        provider: "openai".into(),
        openai_api_key: "old-openai-secret".into(),
        gemini_api_key: "gemini-secret".into(),
        anthropic_api_key: "anthropic-secret".into(),
        ..Default::default()
    };
    let update: PromptConfigUpdate = serde_json::from_value(serde_json::json!({
        "system_prompt": "system", "prompt_template": "{text_snippet}", "model": "gpt-4o-mini",
        "temperature": 0.2, "folder_context_enabled": true, "provider": "openai",
        "custom_base_url": "", "api_key": "new-openai-secret"
    })).unwrap();
    let saved = apply_config_update(existing, update).unwrap();
    assert_eq!(saved.openai_api_key, "new-openai-secret");
    assert_eq!(saved.gemini_api_key, "gemini-secret");
    assert_eq!(saved.anthropic_api_key, "anthropic-secret");
    let public = serde_json::to_string(&PublicPromptConfig::from(&saved)).unwrap();
    assert!(!public.contains("secret"));
    assert!(public.contains("\"openai\":true"));
    assert!(public.contains("\"gemini\":true"));

    let clear: PromptConfigUpdate = serde_json::from_value(serde_json::json!({
        "system_prompt": "system", "prompt_template": "{text_snippet}", "model": "gpt-4o-mini",
        "temperature": 0.2, "folder_context_enabled": true, "provider": "openai",
        "clear_api_key": true
    })).unwrap();
    let cleared = apply_config_update(saved, clear).unwrap();
    assert_eq!(cleared.openai_api_key, "");
    assert_eq!(cleared.gemini_api_key, "gemini-secret");
}

#[test]
fn rejects_unsupported_provider_and_insecure_endpoint_before_saving() {
    for (provider, endpoint) in [("unknown", ""), ("openai", "http://remote.example/request")] {
        let update: PromptConfigUpdate = serde_json::from_value(serde_json::json!({
            "system_prompt": "system", "prompt_template": "prompt", "model": "model",
            "temperature": 0.2, "folder_context_enabled": true,
            "provider": provider, "custom_base_url": endpoint
        })).unwrap();
        assert!(apply_config_update(PromptConfig::default(), update).is_err());
    }
}

#[cfg(unix)]
#[test]
fn saved_config_is_only_readable_by_the_owner() {
    use std::os::unix::fs::PermissionsExt;
    let folder = unique_temp_dir("config-permissions");
    let path = folder.join("config.json");
    save_prompt_config_at(&path, &PromptConfig::default()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    save_prompt_config_at(&path, &PromptConfig::default()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    remove_temp_dir(&folder);
}

#[test]
fn api_keys_round_trip_only_through_ignored_local_file() {
    let folder = unique_temp_dir("config-secrets");
    let path = folder.join("config.json");
    let config = PromptConfig {
        provider: "openai".into(), openai_api_key: "private-key-value".into(),
        ..Default::default()
    };
    save_prompt_config_at(&path, &config).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("private-key-value"));
    assert!(fs::read_to_string(folder.join("config.local.json")).unwrap().contains("private-key-value"));
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(folder.join("config.local.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(load_prompt_config_at(&path).openai_api_key, "private-key-value");
    save_prompt_config_at(&path, &PromptConfig::default()).unwrap();
    assert!(!folder.join("config.local.json").exists());
    remove_temp_dir(&folder);
}

#[test]
fn legacy_inline_keys_migrate_out_of_config_json() {
    let folder = unique_temp_dir("config-migration");
    let path = folder.join("config.json");
    let legacy = PromptConfig {
        openai_api_key: "legacy-key-value".into(), ..Default::default()
    };
    fs::write(&path, serde_json::to_string(&legacy).unwrap()).unwrap();
    assert_eq!(load_prompt_config_at(&path).openai_api_key, "legacy-key-value");
    assert!(!fs::read_to_string(&path).unwrap().contains("legacy-key-value"));
    assert!(folder.join("config.local.json").exists());
    remove_temp_dir(&folder);
}

#[test]
fn malformed_public_config_keeps_saved_api_keys() {
    let folder = unique_temp_dir("config-recovery-keys");
    let path = folder.join("config.json");
    let config = PromptConfig { openai_api_key: "preserve-me".into(), ..Default::default() };
    save_prompt_config_at(&path, &config).unwrap();
    fs::write(&path, "broken JSON").unwrap();
    let recovered = load_prompt_config_at(&path);
    assert_eq!(recovered.openai_api_key, "preserve-me");
    assert!(!fs::read_to_string(&path).unwrap().contains("preserve-me"));
    assert!(folder.join("config.local.json").exists());
    remove_temp_dir(&folder);
}
