//! Agent-specific public configuration fields and native transaction dispatch.
//! Paths come from the installation registry, never from an extension request.
//! Credentials and full document text are not returned to the extension.

use super::config_edit::{self, Format, Patch};
use super::config_transaction::{self, ChangeSet, Store};
use super::{AgentSpec, DiscoveryContext, AGENTS};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadRequest {
    agent_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanRequest {
    agent_id: String,
    changes: Map<String, Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyRequest {
    agent_id: String,
    plan_id: String,
}

struct Field {
    id: &'static str,
    path: Vec<String>,
}
fn field(id: &'static str, path: &[&str]) -> Field {
    Field {
        id,
        path: path.iter().map(|s| s.to_string()).collect(),
    }
}

fn fields(id: &str, values: &Value) -> Vec<Field> {
    match id {
        "codex" => vec![
            field("model", &["model"]),
            field("provider", &["model_provider"]),
            field("effort", &["model_reasoning_effort"]),
            field("serviceTier", &["service_tier"]),
        ],
        "claude" => vec![
            field("model", &["model"]),
            field("effort", &["effortLevel"]),
            field("opusModel", &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"]),
            field("sonnetModel", &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"]),
            field("haikuModel", &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"]),
        ],
        "gemini" | "qwen" => vec![field("model", &["model", "name"])],
        "opencode" => vec![
            field("model", &["model"]),
            field("smallModel", &["small_model"]),
        ],
        "pi" => vec![
            field("model", &["defaultModel"]),
            field("provider", &["defaultProvider"]),
            field("effort", &["defaultThinkingLevel"]),
        ],
        "qoder" | "qoder-cn" => vec![
            field("model", &["model", "name"]),
            field("effort", &["model", "reasoningEffort"]),
        ],
        "grok" => vec![field("model", &["models", "default"])],
        "kimi" => vec![
            field("model", &["default_model"]),
            field("effort", &["default_effort"]),
        ],
        "goose" => vec![
            field("model", &["GOOSE_MODEL"]),
            field("provider", &["GOOSE_PROVIDER"]),
        ],
        "hermes" => vec![field(
            "model",
            if values.get("model").and_then(|v| v.get("default")).is_none()
                && values.get("model").and_then(|v| v.get("model")).is_some()
            {
                &["model", "model"]
            } else {
                &["model", "default"]
            },
        )],
        "droid" => vec![field("model", &["sessionDefaultSettings", "model"])],
        "cline" => {
            let mut result = vec![field("provider", &["lastUsedProvider"])];
            if let Some(provider) = values
                .get("lastUsedProvider")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                result.push(field(
                    "model",
                    &["providers", provider, "settings", "model"],
                ));
            }
            result
        }
        _ => Vec::new(),
    }
}

fn spec(id: &str) -> Result<AgentSpec, String> {
    AGENTS
        .iter()
        .find(|s| s.id == id)
        .copied()
        .ok_or_else(|| "Unknown configuration agent id".into())
}

fn target(context: &DiscoveryContext, spec: AgentSpec) -> Result<(PathBuf, Format), String> {
    let directory = context.root(spec);
    let candidates: Vec<_> = spec
        .configs
        .iter()
        .map(|name| context.config_path(spec, &directory, name))
        .collect();
    let existing: Vec<_> = candidates
        .iter()
        .filter(|p| p.exists() || std::fs::symlink_metadata(p).is_ok())
        .collect();
    // OpenCode's two spellings can both exist; editing just one would report
    // a model saved while the other still wins. Require the ambiguity resolved.
    if existing.len() > 1 {
        return Err("Multiple configuration files exist for this agent; resolve their precedence before changing its settings".into());
    }
    let path = existing
        .first()
        .map(|p| (*p).clone())
        .unwrap_or_else(|| candidates[0].clone());
    let format = match path.extension().and_then(|s| s.to_str()) {
        Some("toml") => Format::Toml,
        Some("yaml" | "yml") => Format::Yaml,
        Some("jsonc") => Format::Jsonc,
        _ => Format::Json,
    };
    Ok((path, format))
}

fn document(
    context: &DiscoveryContext,
    spec: AgentSpec,
) -> Result<(config_transaction::Snapshot, Format, Value), String> {
    let (path, format) = target(context, spec)?;
    let snapshot = config_transaction::snapshot(&path)?;
    let values = config_edit::parse(
        if snapshot.existed {
            &snapshot.bytes
        } else {
            config_edit::default_bytes(format)
        },
        format,
    )?;
    Ok((snapshot, format, values))
}

fn public_read(context: &DiscoveryContext, id: &str) -> Result<Value, String> {
    let (snapshot, format, values) = document(context, spec(id)?)?;
    let warnings = if id == "claude"
        && values
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_MODEL"))
            .is_some()
    {
        vec!["ANTHROPIC_MODEL overrides the default model; changing model includes removing that override"]
    } else {
        Vec::new()
    };
    Ok(json!({"schemaVersion":1,"configuration":{
        "agentId":id,"path":snapshot.source,"format":format,"exists":snapshot.existed,
        "fingerprint":snapshot.fingerprint,"effective":"unverifiedUntilNextSession",
        "fields":fields(id,&values).iter().map(|f| {
            let value = config_edit::get(&values,&f.path);
            json!({"id":f.id,"path":f.path,"value":value.and_then(Value::as_str),"present":value.is_some(),"validType":value.is_none_or(Value::is_string)})
        }).collect::<Vec<_>>(),
        "warnings":warnings,
    }}))
}

fn plan_changes(
    context: &DiscoveryContext,
    id: &str,
    changes: &Map<String, Value>,
) -> Result<ChangeSet, String> {
    if changes.is_empty() || changes.len() > 16 {
        return Err("Select between one and sixteen configuration fields".into());
    }
    let (snapshot, format, values) = document(context, spec(id)?)?;
    let mut proposed = values.clone();
    // Provider selection precedes dependent fields, regardless of JSON order.
    if let Some(provider) = changes.get("provider") {
        if let Some(path) = fields(id, &values)
            .into_iter()
            .find(|f| f.id == "provider")
            .map(|f| f.path)
        {
            if id == "cline"
                && provider
                    .as_str()
                    .is_some_and(|p| values.get("providers").and_then(|v| v.get(p)).is_none())
            {
                return Err("The selected Cline provider is not configured".into());
            }
            if id == "codex"
                && provider.as_str().is_some_and(|p| {
                    p != "openai"
                        && values
                            .get("model_providers")
                            .and_then(|v| v.get(p))
                            .is_none()
                })
            {
                return Err("The selected Codex provider has no configuration definition".into());
            }
            set_semantic(&mut proposed, &path, provider)?;
        }
    }
    let allowed = fields(id, &proposed);
    let mut patches = Vec::new();
    for (key, value) in changes {
        let f = allowed
            .iter()
            .find(|f| f.id == key)
            .ok_or_else(|| format!("Unsupported configuration field: {key}"))?;
        if !value.is_null()
            && !value
                .as_str()
                .is_some_and(|s| !s.is_empty() && s.len() <= 512 && !s.contains(['\r', '\n', '\0']))
        {
            return Err(
                "Agent model settings must be nonempty strings or null for the default".into(),
            );
        }
        patches.push(if value.is_null() {
            Patch::Remove {
                path: f.path.clone(),
            }
        } else {
            Patch::Set {
                path: f.path.clone(),
                value: value.clone(),
            }
        });
        set_semantic(&mut proposed, &f.path, value)?;
    }
    // Remove explicit environment pins that otherwise override the very
    // settings the UI says it changed. The preview includes these removals.
    if id == "claude" {
        for (key, variable) in [
            ("model", "ANTHROPIC_MODEL"),
            ("effort", "CLAUDE_CODE_EFFORT_LEVEL"),
        ] {
            if changes.contains_key(key)
                && values.get("env").and_then(|v| v.get(variable)).is_some()
            {
                patches.push(Patch::Remove {
                    path: vec!["env".into(), variable.into()],
                });
            }
        }
        if let Some(effort) = changes.get("effort") {
            if let Some(model) = proposed.get("model").and_then(Value::as_str) {
                let path = vec!["modelSettings".into(), model.into(), "effortLevel".into()];
                patches.push(if effort.is_null() {
                    Patch::Remove { path }
                } else {
                    Patch::Set {
                        path,
                        value: effort.clone(),
                    }
                });
            }
        }
    }
    if matches!(id, "qoder" | "qoder-cn") {
        if let (Some(effort), Some(model)) = (
            changes.get("effort"),
            proposed
                .get("model")
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str),
        ) {
            let path = vec![
                "model".into(),
                "preferences".into(),
                model.into(),
                "reasoning".into(),
                "effort".into(),
            ];
            patches.push(if effort.is_null() {
                Patch::Remove { path }
            } else {
                Patch::Set {
                    path,
                    value: effort.clone(),
                }
            });
        }
    }
    if id == "pi" && (changes.contains_key("model") || changes.contains_key("provider")) {
        if let Some(scope) = values
            .get("enabledModels")
            .and_then(Value::as_array)
            .filter(|v| !v.is_empty())
        {
            let model = proposed
                .get("defaultModel")
                .and_then(Value::as_str)
                .ok_or("Pi's enabled model scope needs an explicit model")?;
            let provider = proposed
                .get("defaultProvider")
                .and_then(Value::as_str)
                .ok_or("Pi's enabled model scope needs an explicit provider")?;
            let choice = format!("{provider}/{model}");
            if !scope.iter().all(Value::is_string) {
                return Err("Invalid Pi enabledModels configuration".into());
            }
            // Keeping existing patterns and adding the explicit pick is safe
            // even when a vendor's glob syntax is richer than ours.
            if !scope.iter().any(|v| v.as_str() == Some(choice.as_str())) {
                let mut scope = scope.clone();
                scope.push(choice.into());
                patches.push(Patch::Set {
                    path: vec!["enabledModels".into()],
                    value: Value::Array(scope),
                });
            }
        }
    }
    Ok(ChangeSet {
        path: snapshot.source,
        format,
        patches,
    })
}

fn set_semantic(root: &mut Value, path: &[String], value: &Value) -> Result<(), String> {
    let mut parent = root;
    for key in &path[..path.len() - 1] {
        let object = parent
            .as_object_mut()
            .ok_or("Configuration parent is not an object")?;
        if !object.contains_key(key) {
            object.insert(key.clone(), json!({}));
        }
        parent = object.get_mut(key).unwrap();
    }
    let parent = parent
        .as_object_mut()
        .ok_or("Configuration parent is not an object")?;
    if value.is_null() {
        parent.remove(path.last().unwrap());
    } else {
        parent.insert(path.last().unwrap().clone(), value.clone());
    }
    Ok(())
}

pub(super) fn handle(
    operation: &str,
    params: &Value,
    profile: Option<&str>,
) -> Result<Value, String> {
    let context = DiscoveryContext::current()?;
    let root = if let Some(directory) = context
        .environment
        .get("CHROME_USE_RELAY_DIR")
        .filter(|s| !s.is_empty())
    {
        let directory = PathBuf::from(directory);
        if !directory.is_absolute() {
            return Err("CHROME_USE_RELAY_DIR must be absolute for agent panel storage".into());
        }
        directory.join("agent-panel")
    } else {
        context.home.join(".chrome-use/agent-panel")
    };
    match operation {
        "configRead" => {
            let request: ReadRequest = serde_json::from_value(params.clone())
                .map_err(|_| "configRead requires only agentId")?;
            public_read(&context, &request.agent_id)
        }
        "configPlan" => {
            let request: PlanRequest = serde_json::from_value(params.clone())
                .map_err(|_| "configPlan requires agentId and changes")?;
            let store = Store::new(
                root,
                profile.ok_or("Configuration planning requires a bound Chrome profile")?,
            )?;
            let changes = plan_changes(&context, &request.agent_id, &request.changes)?;
            store.plan(&request.agent_id, vec![changes])
        }
        "configApply" | "configRestore" => {
            let request: ApplyRequest = serde_json::from_value(params.clone())
                .map_err(|_| "Configuration apply/restore requires agentId and planId")?;
            let (path, _) = target(&context, spec(&request.agent_id)?)?;
            let store = Store::new(
                root,
                profile.ok_or("Configuration writes require a bound Chrome profile")?,
            )?;
            if operation == "configApply" {
                store.apply(&request.plan_id, &request.agent_id, &[path])
            } else {
                store.restore(&request.plan_id, &request.agent_id, &[path])
            }
        }
        _ => Err("Unsupported configuration operation".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    fn context(home: &TempDir) -> DiscoveryContext {
        DiscoveryContext {
            home: home.path().into(),
            cwd: home.path().into(),
            directories: vec![],
            environment: Default::default(),
            windows: false,
        }
    }
    fn write(home: &TempDir, path: &str, text: &str) {
        let p = home.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    #[test]
    fn reads_only_public_fields_not_inline_tokens_or_raw_source() {
        let home = TempDir::new().unwrap();
        write(&home,".codex/config.toml","model = 'own'\n[model_providers.private]\nexperimental_bearer_token = 'synthetic-secret'\n");
        let read = public_read(&context(&home), "codex").unwrap().to_string();
        assert!(read.contains("own"));
        assert!(!read.contains("synthetic-secret"));
        assert!(!read.contains("experimental_bearer_token"));
    }

    #[test]
    fn malformed_public_fields_cannot_expose_structured_credentials() {
        let home = TempDir::new().unwrap();
        write(
            &home,
            ".claude/settings.json",
            r#"{"model":{"apiKey":"synthetic-hidden"}}"#,
        );
        let result = public_read(&context(&home), "claude").unwrap();
        assert!(!result.to_string().contains("synthetic-hidden"));
        assert_eq!(result["configuration"]["fields"][0]["validType"], false);
    }
    #[test]
    fn claude_plan_removes_environment_pins_and_sets_model_effort() {
        let home = TempDir::new().unwrap();
        write(
            &home,
            ".claude/settings.json",
            r#"{"model":"old","env":{"ANTHROPIC_MODEL":"pinned","ANTHROPIC_AUTH_TOKEN":"synthetic-secret"}}"#,
        );
        let change = plan_changes(
            &context(&home),
            "claude",
            json!({"model":"new.with.dot","effort":"high"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        let raw = std::fs::read(&change.path).unwrap();
        let out = config_edit::patch(&raw, change.format, &change.patches).unwrap();
        let doc = config_edit::parse(&out, change.format).unwrap();
        assert_eq!(doc["model"], "new.with.dot");
        assert!(doc["env"].get("ANTHROPIC_MODEL").is_none());
        assert_eq!(doc["env"]["ANTHROPIC_AUTH_TOKEN"], "synthetic-secret");
        assert_eq!(doc["modelSettings"]["new.with.dot"]["effortLevel"], "high");
    }
    #[test]
    fn cline_provider_change_moves_the_model_in_the_new_provider() {
        let home = TempDir::new().unwrap();
        write(
            &home,
            ".cline/data/settings/providers.json",
            r#"{"lastUsedProvider":"a","providers":{"a":{"settings":{"model":"old-a"}},"b":{"settings":{"model":"old-b"}}}}"#,
        );
        let change = plan_changes(
            &context(&home),
            "cline",
            json!({"model":"new","provider":"b"}).as_object().unwrap(),
        )
        .unwrap();
        let out = config_edit::patch(
            &std::fs::read(&change.path).unwrap(),
            change.format,
            &change.patches,
        )
        .unwrap();
        let doc = config_edit::parse(&out, change.format).unwrap();
        assert_eq!(doc["providers"]["a"]["settings"]["model"], "old-a");
        assert_eq!(doc["providers"]["b"]["settings"]["model"], "new");
        assert_eq!(doc["lastUsedProvider"], "b");
    }
    #[test]
    fn pi_scope_keeps_existing_models_and_includes_the_new_pick() {
        let home = TempDir::new().unwrap();
        write(
            &home,
            ".pi/agent/settings.json",
            r#"{"defaultProvider":"own","defaultModel":"old","enabledModels":["own/old"]}"#,
        );
        let change = plan_changes(
            &context(&home),
            "pi",
            json!({"model":"new"}).as_object().unwrap(),
        )
        .unwrap();
        let out = config_edit::patch(
            &std::fs::read(&change.path).unwrap(),
            change.format,
            &change.patches,
        )
        .unwrap();
        let doc = config_edit::parse(&out, change.format).unwrap();
        assert_eq!(doc["enabledModels"], json!(["own/old", "own/new"]));
    }
    #[test]
    fn unsupported_fields_unknown_providers_and_ambiguous_configs_are_refused() {
        let home = TempDir::new().unwrap();
        let context = context(&home);
        assert!(plan_changes(
            &context,
            "codex",
            json!({"provider":"missing"}).as_object().unwrap()
        )
        .is_err());
        assert!(plan_changes(
            &context,
            "codex",
            json!({"apiKey":"synthetic-secret"}).as_object().unwrap()
        )
        .is_err());
        write(&home, ".config/opencode/opencode.json", "{}");
        write(&home, ".config/opencode/opencode.jsonc", "{}");
        assert!(public_read(&context, "opencode").is_err());
    }
}
