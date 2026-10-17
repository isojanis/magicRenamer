use super::*;
use crate::test_support::*;

#[test]
fn loads_defaults_recovers_malformed_config_and_round_trips_settings() {
    let folder = unique_temp_dir("config");
    let path = folder.join("config.json");
    let defaults = load_prompt_config_at(&path).unwrap();
    assert_eq!(defaults, PromptConfig::default());
    assert!(path.exists());

    fs::write(&path, "not JSON").unwrap();
    assert_eq!(load_prompt_config_at(&path).unwrap(), PromptConfig::default());
    let mut legacy = serde_json::to_value(PromptConfig::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("folder_context_enabled");
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(load_prompt_config_at(&path).unwrap().folder_context_enabled);

    let saved = PromptConfig {
        system_prompt: "system".into(),
        prompt_template: "{text_snippet}:{extension}:{folder_context}".into(),
        model: "test-model".into(),
        temperature: 0.65,
        folder_context_enabled: false,
        ..Default::default()
    };
    save_prompt_config_at(&path, &saved).unwrap();
    assert_eq!(load_prompt_config_at(&path).unwrap(), saved);
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
    assert_eq!(load_prompt_config_at(&path).unwrap().openai_api_key, "private-key-value");
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
    assert_eq!(load_prompt_config_at(&path).unwrap().openai_api_key, "legacy-key-value");
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
    let recovered = load_prompt_config_at(&path).unwrap();
    assert_eq!(recovered.openai_api_key, "preserve-me");
    assert!(!fs::read_to_string(&path).unwrap().contains("preserve-me"));
    assert!(folder.join("config.local.json").exists());
    remove_temp_dir(&folder);
}

#[cfg(unix)]
#[test]
fn settings_reads_and_writes_reject_symlinks_without_touching_the_target() {
    use std::os::unix::fs::symlink;
    let folder = unique_temp_dir("config-symlinks");
    let outside = unique_temp_dir("config-symlink-target");
    let target = outside.join("keep.json");
    fs::write(&target, "protected data").unwrap();
    let config_path = folder.join("config.json");
    symlink(&target, &config_path).unwrap();
    assert!(load_prompt_config_at(&config_path).is_err());
    assert!(save_prompt_config_at(&config_path, &PromptConfig::default()).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "protected data");
    fs::remove_file(&config_path).unwrap();

    fs::write(&config_path, serde_json::to_string(&PromptConfig::default()).unwrap()).unwrap();
    let key_path = folder.join("config.local.json");
    symlink(&target, &key_path).unwrap();
    assert!(load_prompt_config_at(&config_path).is_err());
    let with_key = PromptConfig { openai_api_key: "synthetic-key".into(), ..Default::default() };
    assert!(save_prompt_config_at(&config_path, &with_key).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "protected data");
    remove_temp_dir(&folder);
    remove_temp_dir(&outside);
}

#[test]
fn explicit_import_moves_legacy_settings_and_keys_without_reading_the_working_directory() {
    let legacy = unique_temp_dir("legacy-import-source");
    let private = unique_temp_dir("legacy-import-destination");
    let source = legacy.join("config.json");
    let destination = private.join("config.json");
    let config = PromptConfig {
        provider: "openai".into(), model: "gpt-4o-mini".into(),
        openai_api_key: "old-inline-key".into(), ..Default::default()
    };
    fs::write(&source, serde_json::to_string(&config).unwrap()).unwrap();
    fs::write(legacy.join("config.local.json"), r#"{"openai_api_key":"separate-key"}"#).unwrap();
    let imported = import_prompt_config_at(&source, &destination).unwrap();
    assert_eq!(imported.openai_api_key, "separate-key");
    assert_eq!(load_prompt_config_at(&destination).unwrap().provider, "openai");
    assert!(!fs::read_to_string(&destination).unwrap().contains("separate-key"));
    assert_eq!(fs::read_to_string(&source).unwrap(), serde_json::to_string(&config).unwrap());
    assert!(import_prompt_config_at(&legacy.join("other.json"), &destination).is_err());
    remove_temp_dir(&legacy);
    remove_temp_dir(&private);
}

#[test]
fn malformed_key_file_does_not_rewrite_legacy_inline_credentials() {
    let folder = unique_temp_dir("malformed-keys");
    let path = folder.join("config.json");
    let legacy = PromptConfig { openai_api_key: "legacy-secret".into(), ..Default::default() };
    fs::write(&path, serde_json::to_string(&legacy).unwrap()).unwrap();
    fs::write(folder.join("config.local.json"), "invalid JSON").unwrap();
    assert!(load_prompt_config_at(&path).is_err());
    assert!(fs::read_to_string(&path).unwrap().contains("legacy-secret"));
    remove_temp_dir(&folder);
}
