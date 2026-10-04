use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::browser::BrowserManager;
use crate::connection::config_home;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerCondition {
    pub selector: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiTriggerCondition {
    pub selector: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationStep {
    pub action: String,
    pub selector: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictedAutomation {
    pub id: String,
    pub name: String,
    pub url_pattern: String,
    #[serde(default)]
    pub preconditions: Vec<TriggerCondition>,
    #[serde(default)]
    pub anti_triggers: Vec<AntiTriggerCondition>,
    pub steps: Vec<AutomationStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_condition_selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_condition_url: Option<String>,
    #[serde(default)]
    pub executions: u64,
    #[serde(default)]
    pub divergences: u64,
    #[serde(default)]
    pub tokens_saved_approx: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_executed: Option<String>,
}

pub fn automations_dir() -> PathBuf {
    let dir = config_home().join("automations");
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir
}

pub fn list_automations() -> Result<Vec<PredictedAutomation>, String> {
    let dir = automations_dir();
    let mut results = Vec::new();

    let entries =
        fs::read_dir(&dir).map_err(|e| format!("Failed to read automations directory: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(auto) = serde_json::from_str::<PredictedAutomation>(&content) {
                    results.push(auto);
                }
            }
        }
    }

    results.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(results)
}

pub fn load_automation(id: &str) -> Result<PredictedAutomation, String> {
    let sanitized = sanitize_filename(id);
    let path = automations_dir().join(format!("{sanitized}.json"));
    if !path.exists() {
        return Err(format!("Predicted automation '{id}' not found"));
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read automation '{id}': {e}"))?;
    serde_json::from_str::<PredictedAutomation>(&content)
        .map_err(|e| format!("Invalid automation JSON for '{id}': {e}"))
}

pub fn save_automation(auto: &PredictedAutomation) -> Result<(), String> {
    let sanitized = sanitize_filename(&auto.id);
    let path = automations_dir().join(format!("{sanitized}.json"));
    let content = serde_json::to_string_pretty(auto)
        .map_err(|e| format!("Failed to serialize automation '{}': {e}", auto.id))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write automation '{}': {e}", auto.id))
}

pub fn delete_automation(id: &str) -> Result<bool, String> {
    let sanitized = sanitize_filename(id);
    let path = automations_dir().join(format!("{sanitized}.json"));
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path).map_err(|e| format!("Failed to delete automation '{id}': {e}"))?;
    Ok(true)
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn matches_pattern(url: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern.is_empty() {
        return true;
    }
    if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
        let sub = &pattern[1..pattern.len() - 1];
        return url.contains(sub);
    }
    if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        return url.starts_with(prefix);
    }
    if pattern.starts_with('*') {
        let suffix = &pattern[1..];
        return url.ends_with(suffix);
    }
    url.contains(pattern)
}

pub fn find_matching_automations(url: &str) -> Result<Vec<PredictedAutomation>, String> {
    let all = list_automations()?;
    let matched = all
        .into_iter()
        .filter(|a| matches_pattern(url, &a.url_pattern))
        .collect();
    Ok(matched)
}

pub async fn execute_predicted_automation(
    auto: &mut PredictedAutomation,
    mgr: &mut BrowserManager,
) -> Result<Value, String> {
    let current_url = mgr.get_url().await.unwrap_or_default();
    if !matches_pattern(&current_url, &auto.url_pattern) {
        auto.divergences += 1;
        let _ = save_automation(auto);
        return Ok(json!({
            "success": false,
            "diverged": true,
            "automation_id": auto.id,
            "divergence_type": "url_mismatch",
            "reason": format!("Current URL '{}' does not match pattern '{}'", current_url, auto.url_pattern),
            "current_url": current_url,
            "message": "Page URL diverged from predicted automation pattern. Control yielded to AI.",
        }));
    }

    // 1. In-page trigger evaluation
    let eval_payload = json!({
        "preconditions": auto.preconditions,
        "antiTriggers": auto.anti_triggers,
    });
    let eval_script = format!(
        r#"(function(config) {{
            const preconditions = config.preconditions || [];
            const antiTriggers = config.antiTriggers || [];

            for (let i = 0; i < antiTriggers.length; i++) {{
                const at = antiTriggers[i];
                if (!at || !at.selector) continue;
                try {{
                    const el = document.querySelector(at.selector);
                    if (el) {{
                        const rects = el.getClientRects();
                        const isVisible = el.offsetWidth > 0 || el.offsetHeight > 0 || (rects && rects.length > 0);
                        if (isVisible) {{
                            return {{
                                diverged: true,
                                type: "anti_trigger",
                                reason: at.description || ("Anti-trigger detected: " + at.selector),
                                selector: at.selector
                            }};
                        }}
                    }}
                }} catch (e) {{}}
            }}

            for (let i = 0; i < preconditions.length; i++) {{
                const pre = preconditions[i];
                if (!pre || !pre.selector) continue;
                try {{
                    const el = document.querySelector(pre.selector);
                    if (!el) {{
                        return {{
                            diverged: true,
                            type: "precondition_missing",
                            reason: "Precondition selector not found: " + pre.selector,
                            selector: pre.selector
                        }};
                    }}
                    if (pre.visible) {{
                        const rects = el.getClientRects();
                        const isVisible = el.offsetWidth > 0 || el.offsetHeight > 0 || (rects && rects.length > 0);
                        if (!isVisible) {{
                            return {{
                                diverged: true,
                                type: "precondition_hidden",
                                reason: "Precondition element is hidden: " + pre.selector,
                                selector: pre.selector
                            }};
                        }}
                    }}
                    if (pre.text) {{
                        const text = (el.innerText || el.textContent || "").trim();
                        if (!text.includes(pre.text)) {{
                            return {{
                                diverged: true,
                                type: "precondition_text_mismatch",
                                reason: "Text mismatch on " + pre.selector + ": expected '" + pre.text + "', got '" + text + "'",
                                selector: pre.selector,
                                actualText: text
                            }};
                        }}
                    }}
                }} catch (e) {{
                    return {{
                        diverged: true,
                        type: "precondition_error",
                        reason: "Precondition check threw: " + e.message,
                        selector: pre.selector
                    }};
                }}
            }}

            return {{ diverged: false }};
        }})({})"#,
        serde_json::to_string(&eval_payload).unwrap_or_else(|_| "{}".to_string())
    );

    let check_res = mgr.evaluate(&eval_script, None).await?;
    let diverged = check_res
        .get("diverged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if diverged {
        auto.divergences += 1;
        let _ = save_automation(auto);
        let div_type = check_res
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("trigger_divergence");
        let reason = check_res
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Trigger divergence");
        let selector = check_res.get("selector").and_then(|v| v.as_str());
        return Ok(json!({
            "success": false,
            "diverged": true,
            "automation_id": auto.id,
            "divergence_type": div_type,
            "reason": reason,
            "selector": selector,
            "message": format!("Predicted automation '{}' diverged at precondition/anti-trigger check: {}. Control yielded to AI.", auto.id, reason),
        }));
    }

    // 2. Execute steps
    let steps_count = auto.steps.len();
    for (i, step) in auto.steps.iter().enumerate() {
        let step_payload = serde_json::to_string(step).unwrap_or_else(|_| "{}".to_string());
        let step_script = format!(
            r#"(function(step) {{
                const {{ action, selector, value }} = step;
                if (action === "wait") {{
                    return {{ success: true }};
                }}
                const el = document.querySelector(selector);
                if (!el) {{
                    return {{ success: false, error: "Element not found: " + selector }};
                }}
                try {{
                    if (action === "click") {{
                        el.scrollIntoView({{ block: "nearest", inline: "nearest" }});
                        el.focus();
                        el.dispatchEvent(new MouseEvent("mousedown", {{ bubbles: true, cancelable: true }}));
                        el.dispatchEvent(new MouseEvent("mouseup", {{ bubbles: true, cancelable: true }}));
                        el.click();
                        return {{ success: true }};
                    }}
                    if (action === "fill" || action === "input") {{
                        el.scrollIntoView({{ block: "nearest", inline: "nearest" }});
                        el.focus();
                        const isInput = el instanceof HTMLInputElement;
                        const isTextArea = el instanceof HTMLTextAreaElement;
                        const proto = isTextArea ? HTMLTextAreaElement.prototype : isInput ? HTMLInputElement.prototype : null;
                        const desc = proto ? Object.getOwnPropertyDescriptor(proto, "value") : null;
                        const targetVal = String(value || "");
                        if (desc && desc.set) {{
                            desc.set.call(el, targetVal);
                        }} else {{
                            el.value = targetVal;
                        }}
                        el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                        el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                        return {{ success: true }};
                    }}
                    if (action === "press") {{
                        const key = String(value || "Enter");
                        el.dispatchEvent(new KeyboardEvent("keydown", {{ key, bubbles: true, cancelable: true }}));
                        el.dispatchEvent(new KeyboardEvent("keyup", {{ key, bubbles: true, cancelable: true }}));
                        return {{ success: true }};
                    }}
                    return {{ success: false, error: "Unsupported step action: " + action }};
                }} catch (e) {{
                    return {{ success: false, error: e.message }};
                }}
            }})({})"#,
            step_payload
        );

        if step.action == "wait" {
            let delay = step.wait_ms.unwrap_or(100);
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            continue;
        }

        let exec_res = mgr.evaluate(&step_script, None).await?;
        let step_success = exec_res
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !step_success {
            auto.divergences += 1;
            let _ = save_automation(auto);
            let err_msg = exec_res
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Step failed");
            return Ok(json!({
                "success": false,
                "diverged": true,
                "automation_id": auto.id,
                "divergence_type": "step_execution_failure",
                "step_index": i,
                "failed_action": step.action,
                "selector": step.selector,
                "reason": err_msg,
                "message": format!("Predicted automation '{}' diverged during step {} ({}): {}. Control yielded to AI.", auto.id, i, step.action, err_msg),
            }));
        }
    }

    // 3. Post-condition check
    if let Some(ref post_sel) = auto.post_condition_selector {
        let post_script = format!(
            r#"(function() {{ return !!document.querySelector("{}"); }})()"#,
            post_sel.replace('"', "\\\"")
        );
        let post_res = mgr.evaluate(&post_script, None).await?;
        let exists = post_res.as_bool().unwrap_or(false);
        if !exists {
            auto.divergences += 1;
            let _ = save_automation(auto);
            return Ok(json!({
                "success": false,
                "diverged": true,
                "automation_id": auto.id,
                "divergence_type": "post_condition_missing",
                "selector": post_sel,
                "reason": format!("Post-condition selector '{}' was not found after execution", post_sel),
                "message": format!("Predicted automation '{}' executed steps but post-condition selector was missing. Control yielded to AI.", auto.id),
            }));
        }
    }

    // 4. Update stats
    auto.executions += 1;
    let tokens_saved = (steps_count as u64) * 500;
    auto.tokens_saved_approx += tokens_saved;
    auto.last_executed = Some(
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "now".to_string()),
    );
    let _ = save_automation(auto);

    Ok(json!({
        "success": true,
        "diverged": false,
        "automation_id": auto.id,
        "steps_executed": steps_count,
        "tokens_saved": tokens_saved,
        "total_executions": auto.executions,
        "message": format!("Predicted automation '{}' executed successfully in 1 atomic stroke. Saved ~{} tokens.", auto.id, tokens_saved),
    }))
}
