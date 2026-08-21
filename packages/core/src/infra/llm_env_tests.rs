use super::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());
const LLM_ENV_KEYS: &[&str] = &[
    "REFINE_ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "REFINE_ANTHROPIC_MODEL",
    "REFINE_ANTHROPIC_BASE_URL",
    "ANTHROPIC_BASE_URL",
    "REFINE_OPENAI_API_KEY",
    "OPENAI_API_KEY",
    "REFINE_OPENAI_MODEL",
    "REFINE_OPENAI_BASE_URL",
    "BASE_API_KEY",
    "BASE_MODEL",
    "BASE_URL",
];

#[test]
fn explicit_refine_providers_have_highest_priority() {
    with_clean_llm_env(|| {
        set_env(&[
            ("REFINE_ANTHROPIC_API_KEY", "refine-anthropic-key"),
            ("REFINE_ANTHROPIC_MODEL", "refine-claude"),
            (
                "REFINE_ANTHROPIC_BASE_URL",
                "https://refine-anthropic.example.test",
            ),
            ("REFINE_OPENAI_API_KEY", "refine-openai-key"),
            ("BASE_API_KEY", "base-key"),
            ("BASE_URL", "https://base.example.test"),
            ("ANTHROPIC_AUTH_TOKEN", "ambient-anthropic-key"),
            ("OPENAI_API_KEY", "ambient-openai-key"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity(
                "anthropic",
                "refine-claude",
                "https://refine-anthropic.example.test"
            )
        );
    });

    with_clean_llm_env(|| {
        set_env(&[
            ("REFINE_OPENAI_API_KEY", "refine-openai-key"),
            ("REFINE_OPENAI_MODEL", "refine-openai"),
            ("REFINE_OPENAI_BASE_URL", "https://refine.example.test"),
            ("BASE_API_KEY", "base-key"),
            ("BASE_URL", "https://base.example.test"),
            ("ANTHROPIC_AUTH_TOKEN", "ambient-anthropic-key"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity("openai", "refine-openai", "https://refine.example.test")
        );
    });
}

#[test]
fn complete_base_endpoint_beats_ambient_providers() {
    with_clean_llm_env(|| {
        set_env(&[
            ("BASE_API_KEY", "base-key"),
            ("BASE_MODEL", "base-model"),
            ("BASE_URL", "https://base.example.test/v1"),
            ("ANTHROPIC_AUTH_TOKEN", "ambient-anthropic-key"),
            ("OPENAI_API_KEY", "ambient-openai-key"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity("openai", "base-model", "https://base.example.test")
        );
    });
}

#[test]
fn incomplete_base_endpoint_falls_through() {
    for (key, url) in [
        (Some("base-key"), None),
        (Some("   "), Some("https://base.example.test")),
        (Some("base-key"), Some("   ")),
        (None, Some("https://base.example.test")),
    ] {
        with_clean_llm_env(|| {
            if let Some(key) = key {
                std::env::set_var("BASE_API_KEY", key);
            }
            if let Some(url) = url {
                std::env::set_var("BASE_URL", url);
            }
            std::env::set_var("OPENAI_API_KEY", "ambient-openai-key");

            assert_eq!(
                selected_identity(),
                expected_identity("openai", "gpt-4o", "https://api.openai.com")
            );
        });
    }
}

#[test]
fn incomplete_base_without_an_ambient_provider_is_unusable() {
    for (key, url) in [
        (Some("base-key"), None),
        (Some("   "), Some("https://base.example.test")),
        (None, Some("https://base.example.test")),
    ] {
        with_clean_llm_env(|| {
            if let Some(key) = key {
                std::env::set_var("BASE_API_KEY", key);
            }
            if let Some(url) = url {
                std::env::set_var("BASE_URL", url);
            }

            assert!(build_llm_client_from_env().is_none());
        });
    }
}

#[test]
fn provider_groups_do_not_inherit_lower_priority_fields() {
    with_clean_llm_env(|| {
        set_env(&[
            ("REFINE_OPENAI_API_KEY", "refine-key"),
            ("BASE_MODEL", "base-model"),
            ("BASE_URL", "https://base.example.test"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity("openai", "gpt-4o", "https://api.openai.com")
        );
    });
}

#[test]
fn blank_explicit_keys_fall_through_and_ambient_anthropic_wins_its_tier() {
    with_clean_llm_env(|| {
        set_env(&[
            ("REFINE_ANTHROPIC_API_KEY", " "),
            ("REFINE_OPENAI_API_KEY", ""),
            ("BASE_API_KEY", "base-key"),
            ("BASE_MODEL", "base-model"),
            ("BASE_URL", "https://base.example.test"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity("openai", "base-model", "https://base.example.test")
        );
    });

    with_clean_llm_env(|| {
        set_env(&[
            ("ANTHROPIC_AUTH_TOKEN", "ambient-anthropic-key"),
            ("OPENAI_API_KEY", "ambient-openai-key"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity("anthropic", "claude-opus-4-6", "https://api.anthropic.com")
        );
    });
}

#[test]
fn blank_anthropic_auth_token_does_not_mask_api_key_fallback() {
    with_clean_llm_env(|| {
        set_env(&[
            ("ANTHROPIC_AUTH_TOKEN", " "),
            ("ANTHROPIC_API_KEY", "ambient-api-key"),
            ("OPENAI_API_KEY", "ambient-openai-key"),
        ]);

        assert_eq!(
            selected_identity(),
            expected_identity("anthropic", "claude-opus-4-6", "https://api.anthropic.com")
        );
        assert_eq!(
            ambient_anthropic_config_from_env()
                .expect("ambient Anthropic should be configured")
                .api_key,
            "ambient-api-key"
        );
    });
}

#[test]
fn anthropic_auth_token_wins_over_api_key_when_both_are_set() {
    with_clean_llm_env(|| {
        set_env(&[
            ("ANTHROPIC_AUTH_TOKEN", "auth-token"),
            ("ANTHROPIC_API_KEY", "api-key"),
        ]);

        assert_eq!(
            ambient_anthropic_config_from_env()
                .expect("ambient Anthropic should be configured")
                .api_key,
            "auth-token"
        );
    });
}

fn selected_identity() -> String {
    build_llm_client_from_env()
        .expect("LLM should be configured")
        .cache_identity()
}

fn expected_identity(provider: &str, model: &str, endpoint: &str) -> String {
    format!("{provider}:{model}:{}", endpoint_identity(endpoint))
}

fn set_env(vars: &[(&str, &str)]) {
    for (key, value) in vars {
        std::env::set_var(key, value);
    }
}

fn with_clean_llm_env(test: impl FnOnce()) {
    let Ok(_env_lock) = ENV_LOCK.lock() else {
        panic!("failed to lock LLM environment");
    };
    let guard = LlmEnvGuard::new();
    test();
    drop(guard);
}

struct LlmEnvGuard(Vec<(&'static str, Option<String>)>);

impl LlmEnvGuard {
    fn new() -> Self {
        let saved = LLM_ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();
        for key in LLM_ENV_KEYS {
            std::env::remove_var(key);
        }
        Self(saved)
    }
}

impl Drop for LlmEnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
