use super::*;

fn test_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "vibearound-migration-{}-{}",
        std::process::id(),
        nanoid::nanoid!(8)
    ))
}

#[test]
fn backs_up_then_moves_legacy_state_files_once() {
    let dir = test_dir();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::write(dir.join("workspaces.jsonl"), "legacy-workspaces\n").unwrap();
    std::fs::write(dir.join("workspace-threads.jsonl"), "legacy-threads\n").unwrap();
    std::fs::write(
        dir.join("state/workspace-threads.jsonl"),
        "current-threads\n",
    )
    .unwrap();

    run_at(&dir).unwrap();

    assert!(!dir.join("workspaces.jsonl").exists());
    assert!(!dir.join("workspace-threads.jsonl").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("state/workspaces.jsonl")).unwrap(),
        "legacy-workspaces\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("state/workspace-threads.jsonl")).unwrap(),
        "current-threads\n"
    );

    let backups = backup_dirs(&dir);
    assert_eq!(backups.len(), 1);
    assert!(backups[0].starts_with(
        dir.join("migration-backups")
            .join(format!("v{APPLICATION_VERSION}"))
    ));
    assert_eq!(
        std::fs::read_to_string(backups[0].join("workspaces.jsonl")).unwrap(),
        "legacy-workspaces\n"
    );
    assert_eq!(
        std::fs::read_to_string(backups[0].join("workspace-threads.jsonl")).unwrap(),
        "legacy-threads\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(backups[0].join("workspaces.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    run_at(&dir).unwrap();
    assert_eq!(backup_dirs(&dir).len(), 1);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn backs_up_then_rewrites_settings_aliases_once() {
    let dir = test_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "startkit": { "portableToolchain": true },
            "integrations": {
                "auto_install_mcp": false,
                "auto_install_skills": false
            },
            "proxy": { "url": "http://proxy.test" },
            "bridge": {
                "replaceProviderWebSearch": true,
                "rate_limit_retry": { "delay": 7, "retries": 3 }
            },
            "localAgentApi": { "enabled": true },
            "searchTool": {
                "command": "/tmp/search",
                "maxResults": 8,
                "searchContextSize": "high",
                "sources": {
                    "exa": {
                        "apiKey": "secret",
                        "apiKeyEnv": "EXA_KEY",
                        "baseUrl": "https://example.test"
                    }
                }
            },
            "serviceSide": {
                "imageInput": {
                    "enabled": true,
                    "profileId": "vision",
                    "apiType": "openai-chat",
                    "model": "vision-model"
                }
            },
            "im_remote": {
                "channels": {
                    "telegram": {
                        "agent": "codex",
                        "profileId": "direct",
                        "workspacePath": "/tmp/legacy-workspace",
                        "unknown": true
                    }
                }
            },
            "launcher": { "workspace": "/tmp/legacy-launcher-workspace" },
            "working_dir": "/tmp/legacy-default-workspace",
            "unknown_root": true
        }))
        .unwrap(),
    )
    .unwrap();

    run_at(&dir).unwrap();

    let body = std::fs::read_to_string(&path).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&body).unwrap();
    let config = crate::config::config_from_settings_json(&settings);
    assert!(config.portable_toolchain);
    assert!(settings.get("integrations").is_none());
    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://proxy.test")
    );
    assert!(config.api_bridge.replace_provider_web_search);
    assert_eq!(config.api_bridge.retry_429.delay_seconds, 7);
    assert_eq!(config.api_bridge.retry_429.max_retries, Some(3));
    assert!(config.local_agent_api.enabled);
    assert_eq!(config.search_tool.max_results, Some(8));
    assert_eq!(
        config.search_tool.sources["exa"].api_key.as_deref(),
        Some("secret")
    );
    assert_eq!(
        config.service_side.image_input.profile_id.as_deref(),
        Some("vision")
    );
    assert_eq!(
        config.remote.channels["telegram"].agent_id.as_deref(),
        Some("codex")
    );
    assert_eq!(settings["remote"]["channels"]["telegram"]["unknown"], true);
    assert!(settings["remote"]["channels"]["telegram"]
        .get("workspacePath")
        .is_none());
    assert_eq!(settings["unknown_root"], true);
    assert!(settings.get("bridge").is_none());
    assert!(settings.get("searchTool").is_none());
    assert!(settings.get("working_dir").is_none());
    assert_eq!(
        settings["default_workspace"],
        "/tmp/legacy-launcher-workspace"
    );
    assert!(settings["launcher"].get("workspace").is_none());
    assert!(settings["api_bridge"].get("rate_limit_retry").is_none());
    assert!(settings["search_tool"]["sources"]["exa"]
        .get("apiKey")
        .is_none());

    let backups = backup_dirs(&dir);
    assert_eq!(backups.len(), 1);
    assert!(std::fs::read_to_string(backups[0].join("settings.json"))
        .unwrap()
        .contains("\"bridge\""));
    run_at(&dir).unwrap();
    assert_eq!(backup_dirs(&dir).len(), 1);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn canonical_settings_values_win_over_aliases() {
    let mut settings = serde_json::json!({
        "default_workspace": "/tmp/canonical-workspace",
        "launcher": { "workspace": "/tmp/legacy-workspace" },
        "startkit": {
            "portable_toolchain": false,
            "portableToolchain": true
        }
    });

    assert!(canonicalize_settings(&mut settings));

    assert_eq!(settings["startkit"]["portable_toolchain"], false);
    assert!(settings["startkit"].get("portableToolchain").is_none());
    assert_eq!(settings["default_workspace"], "/tmp/canonical-workspace");
    assert!(settings["launcher"].get("workspace").is_none());
}

#[test]
fn materializes_the_legacy_managed_toolchain_default() {
    let mut settings = serde_json::json!({
        "startkit": { "toolchain_mode": "managed" }
    });

    assert!(canonicalize_settings(&mut settings));

    assert_eq!(settings["startkit"]["portable_toolchain"], true);
}

#[test]
fn backup_failure_keeps_original_files_and_does_not_fail_startup() {
    let dir = test_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let original = r#"{ "bridge": { "replaceProviderWebSearch": true } }"#;
    std::fs::write(&path, original).unwrap();
    std::fs::write(dir.join("migration-backups"), "not a directory").unwrap();

    run_at(&dir).unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn backs_up_then_rewrites_legacy_provider_profiles_once() {
    let dir = test_dir();
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    let legacy_path = dir.join("profiles/qwen-old.json");
    std::fs::write(
        &legacy_path,
        r#"{
  "id": "qwen-old",
  "label": "Qwen / DashScope",
  "provider": "qwen",
  "auth_mode": "api_key",
  "api_types": ["openai-chat"],
  "credentials": { "api_key": "secret" },
  "overrides": {
    "openai-chat": { "endpoint_id": "standard-cn" }
  }
}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("profiles/current.json"),
        r#"{
  "id": "current",
  "label": "Current",
  "provider": "deepseek",
  "auth_mode": "api_key",
  "api_configs": {
    "openai-chat": { "enabled": true }
  }
}"#,
    )
    .unwrap();

    run_at(&dir).unwrap();

    let migrated: crate::profiles::ProfileDef =
        serde_json::from_str(&std::fs::read_to_string(&legacy_path).unwrap()).unwrap();
    assert_eq!(migrated.provider, "dashscope");
    assert_eq!(migrated.label, "Alibaba DashScope");
    assert_eq!(
        migrated.api_configs["openai-chat"].endpoint_id.as_deref(),
        Some("token-plan-cn")
    );
    assert_eq!(migrated.credentials["api_key"], "secret");
    let migrated_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&legacy_path).unwrap()).unwrap();
    assert!(migrated_json.get("api_types").is_none());
    assert!(migrated_json.get("overrides").is_none());

    let backups = backup_dirs(&dir);
    assert_eq!(backups.len(), 1);
    let original = std::fs::read_to_string(backups[0].join("profiles/qwen-old.json")).unwrap();
    assert!(original.contains("\"provider\": \"qwen\""));
    assert!(!backups[0].join("profiles/current.json").exists());

    run_at(&dir).unwrap();
    assert_eq!(backup_dirs(&dir).len(), 1);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreadable_profile_does_not_block_other_profile_migrations() {
    let dir = test_dir();
    let profiles_dir = dir.join("profiles");
    std::fs::create_dir_all(profiles_dir.join("unreadable.json")).unwrap();
    let legacy_path = profiles_dir.join("deepseek-old.json");
    std::fs::write(
        &legacy_path,
        r#"{
  "id": "deepseek-old",
  "label": "DeepSeek Old",
  "provider": "deepseek",
  "auth_mode": "api_key",
  "api_types": ["openai-chat"]
}"#,
    )
    .unwrap();

    run_at(&dir).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(legacy_path).unwrap()).unwrap();
    assert!(migrated.get("api_types").is_none());
    assert!(migrated["api_configs"]["openai-chat"]["enabled"]
        .as_bool()
        .unwrap());
    assert!(profiles_dir.join("unreadable.json").is_dir());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unmappable_legacy_api_type_keeps_the_original_profile() {
    let dir = test_dir();
    let profiles_dir = dir.join("profiles");
    std::fs::create_dir_all(&profiles_dir).unwrap();
    let path = profiles_dir.join("partial.json");
    let original = r#"{
  "id": "partial",
  "label": "Partial",
  "provider": "deepseek",
  "auth_mode": "api_key",
  "api_types": ["openai-chat", "unsupported"]
}"#;
    std::fs::write(&path, original).unwrap();

    run_at(&dir).unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert!(!dir.join("migration-backups").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_migrated_profile_keeps_the_original_file() {
    let dir = test_dir();
    let profiles_dir = dir.join("profiles");
    std::fs::create_dir_all(&profiles_dir).unwrap();
    let path = profiles_dir.join("invalid.json");
    let original = r#"{
  "id": "invalid",
  "label": "",
  "provider": "deepseek",
  "auth_mode": "api_key",
  "api_types": ["openai-chat"]
}"#;
    std::fs::write(&path, original).unwrap();

    run_at(&dir).unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert!(!dir.join("migration-backups").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migrates_single_bridge_model_fields_into_models() {
    let dir = test_dir();
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    let path = dir.join("profiles/bridge-old.json");
    std::fs::write(
        &path,
        r#"{
  "id": "bridge-old",
  "label": "Bridge Old",
  "provider": "custom",
  "auth_mode": "api_key",
  "api_configs": {
    "openai-chat": {
      "enabled": true,
      "model": "provider-default"
    }
  },
  "connections": {
    "claude": {
      "selectedApiType": "anthropic",
      "bridge": {
        "anthropic": {
          "enabled": true,
          "targetApiType": "openai-chat",
          "upstreamModel": "provider-model",
          "fakeModelId": "claude-sonnet-4-5"
        }
      }
    }
  }
}"#,
    )
    .unwrap();

    run_at(&dir).unwrap();

    let body = std::fs::read_to_string(&path).unwrap();
    let profile: crate::profiles::ProfileDef = serde_json::from_str(&body).unwrap();
    let bridge = &profile.connections["claude"].bridge["anthropic"];
    assert_eq!(bridge.models.len(), 1);
    assert_eq!(
        bridge.models[0].upstream_model.as_deref(),
        Some("provider-model")
    );
    assert_eq!(
        bridge.models[0].fake_model_id.as_deref(),
        Some("claude-sonnet-4-5")
    );
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bridge_json = &json["connections"]["claude"]["bridge"]["anthropic"];
    assert!(bridge_json.get("upstreamModel").is_none());
    assert!(bridge_json.get("fakeModelId").is_none());
    assert_eq!(backup_dirs(&dir).len(), 1);

    run_at(&dir).unwrap();
    assert_eq!(backup_dirs(&dir).len(), 1);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migrates_connection_proxy_to_bridge_once() {
    let dir = test_dir();
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    let path = dir.join("profiles/proxy-old.json");
    std::fs::write(
        &path,
        r#"{
  "id": "proxy-old",
  "label": "Proxy Old",
  "provider": "custom",
  "auth_mode": "api_key",
  "api_configs": {
    "openai-chat": { "enabled": true, "model": "provider-default" }
  },
  "connections": {
    "claude": {
      "proxy": {
        "anthropic": { "enabled": true, "targetApiType": "openai-chat" }
      }
    },
    "codex": {
      "bridge": {
        "openai-responses": { "enabled": true, "targetApiType": "openai-chat" }
      },
      "proxy": {
        "ignored": { "enabled": true }
      }
    }
  }
}"#,
    )
    .unwrap();

    run_at(&dir).unwrap();

    let body = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["connections"]["claude"].get("proxy").is_none());
    assert!(json["connections"]["claude"]["bridge"]["anthropic"].is_object());
    assert!(json["connections"]["codex"].get("proxy").is_none());
    assert!(json["connections"]["codex"]["bridge"]["openai-responses"].is_object());
    assert!(json["connections"]["codex"]["bridge"]
        .get("ignored")
        .is_none());
    assert_eq!(backup_dirs(&dir).len(), 1);

    run_at(&dir).unwrap();
    assert_eq!(backup_dirs(&dir).len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn numbers_each_backup_for_the_application_version_and_local_date() {
    let dir = test_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("workspaces.jsonl"), "legacy-workspaces\n").unwrap();
    run_at(&dir).unwrap();

    std::fs::write(
        dir.join("workspace-threads.jsonl"),
        "legacy-workspace-threads\n",
    )
    .unwrap();
    run_at(&dir).unwrap();

    let backups = backup_dirs(&dir);
    assert_eq!(backups.len(), 2);
    assert_eq!(backups[0].file_name().unwrap(), "001");
    assert_eq!(backups[1].file_name().unwrap(), "002");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migrates_legacy_azure_api_type() {
    let mut profile: MigrationProfile = serde_json::from_value(serde_json::json!({
        "id": "azure-old",
        "label": "Azure",
        "provider": "azure",
        "auth_mode": "api_key",
        "api_types": ["openai-chat"],
        "overrides": {
            "openai-chat": { "model": "gpt-5" }
        }
    }))
    .unwrap();

    assert!(migrate_legacy_profile_provider(&mut profile));
    hydrate_legacy_api_configs(&mut profile).unwrap();
    let profile = profile.into_profile();
    assert_eq!(
        profile.api_configs["openai-responses"].model.as_deref(),
        Some("gpt-5")
    );
}

#[test]
fn normalizes_legacy_provider_fields_before_hydration() {
    let mut kimi: MigrationProfile = serde_json::from_value(serde_json::json!({
        "id": "kimi-old",
        "label": "Kimi Coding",
        "provider": "kimi",
        "auth_mode": "api_key",
        "api_types": ["anthropic"],
        "overrides": {
            "anthropic": {
                "base_url": "https://api.kimi.com/coding/",
                "model": "kimi-for-coding"
            }
        }
    }))
    .unwrap();
    assert!(migrate_legacy_profile_provider(&mut kimi));
    hydrate_legacy_api_configs(&mut kimi).unwrap();
    let kimi = kimi.into_profile();
    assert_eq!(kimi.provider, "moonshot");
    assert_eq!(
        kimi.api_configs["anthropic"].endpoint_id.as_deref(),
        Some("kimi-coding")
    );

    let mut gemini: MigrationProfile = serde_json::from_value(serde_json::json!({
        "id": "gemini-old",
        "label": "Gemini",
        "provider": "gemini",
        "auth_mode": "oauth_via_cli",
        "api_types": ["openai-chat"],
        "overrides": {
            "openai-chat": { "endpoint_id": "openai-compatible" }
        }
    }))
    .unwrap();
    assert!(migrate_legacy_profile_provider(&mut gemini));
    hydrate_legacy_api_configs(&mut gemini).unwrap();
    let gemini = gemini.into_profile();
    assert_eq!(gemini.auth_mode, crate::profiles::AuthMode::GoogleOauth);
    assert_eq!(
        gemini.api_configs["openai-chat"].endpoint_id.as_deref(),
        Some("gemini-api")
    );
}

#[test]
fn hydrates_legacy_custom_profile_api_configs() {
    let mut profile: MigrationProfile = serde_json::from_value(serde_json::json!({
        "id": "sensenova",
        "label": "SenseNova",
        "provider": "custom",
        "auth_mode": "api_key",
        "api_types": ["anthropic", "openai-chat"],
        "overrides": {
            "anthropic": {
                "base_url": "https://token.sensenova.cn",
                "model": "sensenova-6.7-flash-lite"
            },
            "openai-chat": {
                "base_url": "https://token.sensenova.cn/v1",
                "model": "sensenova-6.7-flash-lite"
            }
        }
    }))
    .unwrap();

    hydrate_legacy_api_configs(&mut profile).unwrap();
    let profile = profile.into_profile();
    assert_eq!(
        profile.api_configs["anthropic"].base_url.as_deref(),
        Some("https://token.sensenova.cn")
    );
    assert!(profile.api_configs["anthropic"].models[0].custom);
    assert_eq!(
        profile.api_configs["openai-chat"].base_url.as_deref(),
        Some("https://token.sensenova.cn/v1")
    );
}

fn backup_dirs(data_dir: &Path) -> Vec<PathBuf> {
    let version_dir = data_dir
        .join("migration-backups")
        .join(format!("v{APPLICATION_VERSION}"));
    let mut dirs = std::fs::read_dir(version_dir)
        .unwrap()
        .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}
