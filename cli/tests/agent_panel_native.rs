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
