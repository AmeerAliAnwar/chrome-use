//! Real framed native-host acceptance, without Chrome or an agent process.
//! Run through scripts/remote-cargo.py on the configured build host. The relay
//! registry is temporary; discovery reads installation metadata only.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct Host(Child);

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn send(host: &mut Host, value: Value) {
    let data = serde_json::to_vec(&value).unwrap();
    let input = host.0.stdin.as_mut().unwrap();
    input.write_all(&(data.len() as u32).to_ne_bytes()).unwrap();
    input.write_all(&data).unwrap();
    input.flush().unwrap();
}

fn frames(host: &mut Host) -> mpsc::Receiver<Result<Value, String>> {
    let mut stdout = host.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || loop {
        let mut length = [0u8; 4];
        if stdout.read_exact(&mut length).is_err() {
            break;
        }
        let length = u32::from_ne_bytes(length) as usize;
        if length > 1024 * 1024 {
            let _ = tx.send(Err("Native stdout was not a bounded frame".into()));
            break;
        }
        let mut raw = vec![0u8; length];
        if stdout.read_exact(&mut raw).is_err() {
            break;
        }
        if tx
            .send(serde_json::from_slice(&raw).map_err(|_| "Invalid native JSON frame".into()))
            .is_err()
        {
            break;
        }
    });
    rx
}

fn panel_reply(
    host: &mut Host,
    rx: &mpsc::Receiver<Result<Value, String>>,
    request_id: &str,
    operation: &str,
    params: Value,
) -> Value {
    send(
        host,
        json!({"method":"agentPanel","requestId":request_id,"operation":operation,"params":params}),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let reply = rx
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .expect("Native panel service did not answer")
            .unwrap();
        if reply["method"] == "ping" {
            send(host, json!({"method":"pong"}));
            continue;
        }
        assert_eq!(
            reply["method"], "agentPanelReply",
            "Panel request entered browser command dispatch"
        );
        assert_eq!(reply["requestId"], request_id);
        return reply;
    }
}

#[test]
fn configuration_preview_apply_and_restore_use_only_isolated_fixture_settings() {
    let registry = tempfile::tempdir().unwrap();
    let agent_dir = registry.path().join("synthetic-agent");
    std::fs::create_dir(&agent_dir).unwrap();
    let settings = agent_dir.join("settings.json");
    let original = br#"{"model":"old","env":{"ANTHROPIC_MODEL":"pinned","ANTHROPIC_AUTH_TOKEN":"synthetic-token"},"theme":"light"}"#;
    std::fs::write(&settings, original).unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_chrome-use"))
            .args(["__nm-host", "chrome-extension://agent-panel-fixture/"])
            .env("CHROME_USE_RELAY_DIR", registry.path())
            .env("CLAUDE_CONFIG_DIR", &agent_dir)
            .env("CHROME_USE_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let rx = frames(&mut host);
    let unbound = panel_reply(
        &mut host,
        &rx,
        "unbound",
        "configPlan",
        json!({"agentId":"claude","changes":{"model":"new"}}),
    );
    assert!(unbound["error"]
        .as_str()
        .unwrap()
        .contains("bound Chrome profile"));
    assert_eq!(std::fs::read(&settings).unwrap(), original);
    send(
        &mut host,
        json!({"method":"hello","version":"fixture","profileId":"native-config-fixture"}),
    );
    let read = panel_reply(
        &mut host,
        &rx,
        "read",
        "configRead",
        json!({"agentId":"claude"}),
    );
    assert!(read.get("error").is_none());
    assert_eq!(read["result"]["configuration"]["agentId"], "claude");
    assert!(!read.to_string().contains("synthetic-token"));
    let planned = panel_reply(
        &mut host,
        &rx,
        "preview",
        "configPlan",
        json!({"agentId":"claude","changes":{"model":"new","effort":"high"}}),
    );
    assert!(planned.get("error").is_none());
    assert_eq!(planned["result"]["configuration"]["phase"], "planned");
    assert!(!planned.to_string().contains("synthetic-token"));
    assert_eq!(std::fs::read(&settings).unwrap(), original);
    let plan = planned["result"]["configuration"]["planId"]
        .as_str()
        .unwrap();
    assert!(registry
        .path()
        .join(format!(
            "agent-panel/profiles/native-config-fixture/configuration/{plan}.json"
        ))
        .is_file());
    let applied = panel_reply(
        &mut host,
        &rx,
        "apply",
        "configApply",
        json!({"agentId":"claude","planId":plan}),
    );
    assert_eq!(applied["result"]["configuration"]["phase"], "applied");
    assert!(!applied.to_string().contains("synthetic-token"));
    let current: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(current["model"], "new");
    assert_eq!(current["effortLevel"], "high");
    assert_eq!(current["env"]["ANTHROPIC_AUTH_TOKEN"], "synthetic-token");
    assert!(current["env"].get("ANTHROPIC_MODEL").is_none());
    let restored = panel_reply(
        &mut host,
        &rx,
        "restore",
        "configRestore",
        json!({"agentId":"claude","planId":plan}),
    );
    assert_eq!(restored["result"]["configuration"]["phase"], "restored");
    assert_eq!(std::fs::read(&settings).unwrap(), original);
    let invalid = panel_reply(
        &mut host,
        &rx,
        "unknown-field",
        "configPlan",
        json!({"agentId":"claude","changes":{"apiKey":"synthetic-new-secret"}}),
    );
    assert!(invalid.get("error").is_some());
    assert_eq!(std::fs::read(&settings).unwrap(), original);
}

#[test]
fn discovery_and_errors_are_correlated_without_entering_browser_dispatch() {
    let registry = tempfile::tempdir().unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_chrome-use"))
            .args(["__nm-host", "chrome-extension://agent-panel-fixture/"])
            .env("CHROME_USE_RELAY_DIR", registry.path())
            .env("CHROME_USE_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut stdout = host.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<Result<Value, String>>();
    std::thread::spawn(move || loop {
        let mut length = [0u8; 4];
        if stdout.read_exact(&mut length).is_err() {
            break;
        }
        let length = u32::from_ne_bytes(length) as usize;
        if length > 1024 * 1024 {
            let _ = tx.send(Err("Native stdout was not a bounded framed message".into()));
            break;
        }
        let mut data = vec![0u8; length];
        if stdout.read_exact(&mut data).is_err() {
            break;
        }
        let value = serde_json::from_slice(&data).map_err(|_| "Invalid native JSON frame".into());
        if tx.send(value).is_err() {
            break;
        }
    });

    send(
        &mut host,
        json!({"method": "agentPanel", "requestId": "discovery", "operation": "discover", "params": {}}),
    );
    send(&mut host, json!({"method": "ping"}));
    send(
        &mut host,
        json!({"method": "agentPanel", "requestId": "unknown", "operation": "run-arbitrary", "params": {}}),
    );
    send(
        &mut host,
        json!({"method": "agentPanel", "requestId": "malformed", "operation": "discover", "params": {"arbitrary": "value"}}),
    );

    let mut discovery = false;
    let mut pong = false;
    let mut unknown = false;
    let mut malformed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !(discovery && pong && unknown && malformed) {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let frame = rx
            .recv_timeout(remaining)
            .expect("Native host did not answer before the deadline")
            .unwrap();
        match frame["method"].as_str() {
            Some("pong") => pong = true,
            Some("agentPanelReply") => match frame["requestId"].as_str() {
                Some("discovery") => {
                    assert!(frame.get("error").is_none());
                    assert_eq!(frame["result"]["schemaVersion"], 1);
                    let agents = frame["result"]["agents"].as_array().unwrap();
                    assert!(agents.iter().any(|a| a["id"] == "codex"));
                    assert!(agents.iter().any(|a| a["id"] == "claude"));
                    for agent in agents {
                        assert_eq!(agent["authentication"], "unverified");
                        assert_eq!(agent["runtime"], "unverified");
                        assert!(agent.get("credentials").is_none());
                        assert!(agent.get("token").is_none());
                    }
                    discovery = true;
                }
                Some("unknown") => {
                    assert!(frame["error"].as_str().unwrap().contains("Unsupported"));
                    assert!(frame.get("result").is_none());
                    unknown = true;
                }
                Some("malformed") => {
                    assert!(frame["error"].as_str().unwrap().contains("empty params"));
                    assert!(frame.get("result").is_none());
                    malformed = true;
                }
                _ => panic!("Reply has an unknown request identity"),
            },
            Some("ping") => send(&mut host, json!({"method": "pong"})),
            _ => panic!("Panel work entered the browser dispatcher or polluted protocol stdout"),
        }
    }
    drop(host.0.stdin.take());
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = host.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            std::time::Instant::now() < exit_deadline,
            "Native host did not exit after EOF"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
