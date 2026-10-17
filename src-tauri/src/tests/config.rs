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
