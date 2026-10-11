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

#[cfg(unix)]
struct ServiceFixture {
    root: std::path::PathBuf,
    profile: String,
    chats: Vec<String>,
}

#[cfg(unix)]
impl ServiceFixture {
    fn request(&self, operation: &str, params: Value) -> Option<Value> {
        use std::net::TcpStream;
        let endpoint: Value = serde_json::from_slice(
            &std::fs::read(self.root.join(format!(
                "agent-panel/profiles/{}/service.json",
                self.profile
            )))
            .ok()?,
        )
        .ok()?;
        if endpoint["profile"] != self.profile {
            return None;
        }
        let mut stream = TcpStream::connect_timeout(
            &endpoint["address"].as_str()?.parse().ok()?,
            Duration::from_millis(300),
        )
        .ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        let id = uuid::Uuid::new_v4().to_string();
        let raw=json!({"capability":endpoint["capability"],"requestId":id,"operation":operation,"params":params}).to_string().into_bytes();
        stream.write_all(&(raw.len() as u32).to_le_bytes()).ok()?;
        stream.write_all(&raw).ok()?;
        let mut len = [0u8; 4];
        stream.read_exact(&mut len).ok()?;
        let len = u32::from_le_bytes(len) as usize;
        if len > 768 * 1024 {
            return None;
        }
        let mut raw = vec![0u8; len];
        stream.read_exact(&mut raw).ok()?;
        let reply: Value = serde_json::from_slice(&raw).ok()?;
        if reply["requestId"] != id {
            return None;
        }
        Some(reply)
    }
}

#[cfg(unix)]
impl Drop for ServiceFixture {
    fn drop(&mut self) {
        for id in &self.chats {
            let _ = self.request("conversationStop", json!({"conversationId":id}));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let Some(health) = self.request("health", json!({})) else {
                break;
            };
            if health["result"]["active"] == 0 {
                let _ = self.request("serviceStop", json!({}));
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(unix)]
fn conversation_poll(
    host: &mut Host,
    rx: &mpsc::Receiver<Result<Value, String>>,
    id: &str,
    phase: &str,
) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let reply = panel_reply(
            host,
            rx,
            "poll-conversation",
            "conversationRead",
            json!({"conversationId":id,"after":0}),
        );
        assert!(
            reply.get("error").is_none(),
            "Conversation read failed: {}",
            reply["error"]
        );
        if reply["result"]["conversation"]["phase"] == phase {
            return reply["result"].clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Conversation did not reach {phase}; observed {}",
            reply["result"]["conversation"]["phase"]
        );
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[test]
#[cfg(unix)]
fn both_cli_protocols_keep_conversations_alive_across_native_host_disconnects() {
    let registry = tempfile::tempdir().unwrap();
    let profile = "native-conversation-fixture";
    let mut cleanup = ServiceFixture {
        root: registry.path().into(),
        profile: profile.into(),
        chats: vec![],
    };
    let spawn = || {
        Host(
            Command::new(env!("CARGO_BIN_EXE_chrome-use"))
                .args(["__nm-host", "chrome-extension://agent-panel-fixture/"])
                .env("CHROME_USE_RELAY_DIR", registry.path())
                .env("CHROME_USE_NO_UPDATE_CHECK", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let mut host = spawn();
    let mut rx = frames(&mut host);
    send(
        &mut host,
        json!({"method":"hello","version":"fixture","profileId":profile}),
    );
    let health = panel_reply(&mut host, &rx, "health", "serviceHealth", json!({}));
    assert!(health.get("error").is_none());
    let service_pid = health["result"]["pid"].clone();
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/panel_agent.py");
    for agent in ["codex", "claude"] {
        let created = panel_reply(
            &mut host,
            &rx,
            "create",
            "conversationCreate",
            json!({"agentId":agent,"executable":fixture}),
        );
        assert!(created.get("error").is_none());
        let id = created["result"]["conversation"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        cleanup.chats.push(id.clone());
        let message = uuid::Uuid::new_v4().to_string();
        let sent = panel_reply(
            &mut host,
            &rx,
            "send-first",
            "conversationSend",
            json!({"conversationId":id,"messageId":message,"text":"first"}),
        );
        assert!(sent.get("error").is_none());
        let done = conversation_poll(&mut host, &rx, &id, "idle");
        assert_eq!(
            done["conversation"]["model"],
            format!("synthetic-{agent}-model")
        );
        let repeated = panel_reply(
            &mut host,
            &rx,
            "send-duplicate",
            "conversationSend",
            json!({"conversationId":id,"messageId":message,"text":"first"}),
        );
        assert_eq!(repeated["result"]["message"]["accepted"], false);
        let status = panel_reply(
            &mut host,
            &rx,
            "message-status",
            "conversationMessageStatus",
            json!({"conversationId":id,"messageId":message}),
        );
        assert_eq!(status["result"]["message"]["id"], message);
        assert_eq!(status["result"]["message"]["known"], true);
        assert_eq!(status["result"]["message"]["status"], "previouslyAccepted");
        let unknown = panel_reply(
            &mut host,
            &rx,
            "unknown-message-status",
            "conversationMessageStatus",
            json!({"conversationId":id,"messageId":uuid::Uuid::new_v4().to_string()}),
        );
        assert_eq!(unknown["result"]["message"]["known"], false);
        assert_eq!(unknown["result"]["message"]["status"], "unconfirmed");
        let history = conversation_poll(&mut host, &rx, &id, "idle");
        assert_eq!(
            history["journal"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "user")
                .count(),
            1
        );
        let waiting = uuid::Uuid::new_v4().to_string();
        panel_reply(
            &mut host,
            &rx,
            "send-wait",
            "conversationSend",
            json!({"conversationId":id,"messageId":waiting,"text":"wait"}),
        );
        conversation_poll(&mut host, &rx, &id, "running");
        drop(host);
        host = spawn();
        rx = frames(&mut host);
        send(
            &mut host,
            json!({"method":"hello","version":"fixture","profileId":profile}),
        );
        let reconnected = panel_reply(
            &mut host,
            &rx,
            "health-reconnect",
            "serviceHealth",
            json!({}),
        );
        assert_eq!(reconnected["result"]["pid"], service_pid);
        let observed = conversation_poll(&mut host, &rx, &id, "running");
        assert_eq!(observed["conversation"]["activeMessageId"], waiting);
        panel_reply(
            &mut host,
            &rx,
            "stop",
            "conversationStop",
            json!({"conversationId":id}),
        );
        conversation_poll(&mut host, &rx, &id, "interrupted");
        panel_reply(
            &mut host,
            &rx,
            "resume",
            "conversationResumeQueue",
            json!({"conversationId":id}),
        );
        panel_reply(
            &mut host,
            &rx,
            "send-approval",
            "conversationSend",
            json!({"conversationId":id,"messageId":uuid::Uuid::new_v4().to_string(),"text":"approval"}),
        );
        let blocked = conversation_poll(&mut host, &rx, &id, "waitingInput");
        let request = blocked["journal"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["kind"] == "request")
            .unwrap()["data"]["requestId"]
            .clone();
        let answer = if agent == "codex" {
            json!({"decision":"accept"})
        } else {
            json!({"behavior":"allow","updatedInput":{"questions":[],"answers":{"Synthetic question":"One"}}})
        };
        let reply = panel_reply(
            &mut host,
            &rx,
            "answer",
            "conversationRespond",
            json!({"conversationId":id,"requestId":request,"result":answer}),
        );
        assert_eq!(reply["result"]["response"]["confirmed"], false);
        conversation_poll(&mut host, &rx, &id, "idle");
        panel_reply(
            &mut host,
            &rx,
            "send-fail",
            "conversationSend",
            json!({"conversationId":id,"messageId":uuid::Uuid::new_v4().to_string(),"text":"fail"}),
        );
        let queued_id = uuid::Uuid::new_v4().to_string();
        panel_reply(
            &mut host,
            &rx,
            "queue-next",
            "conversationSend",
            json!({"conversationId":id,"messageId":queued_id,"text":"queued-after-failure"}),
        );
        let failed = conversation_poll(&mut host, &rx, &id, "failed");
        assert_eq!(failed["conversation"]["queuePaused"], true);
        assert_eq!(failed["conversation"]["queued"], 1);
        assert_eq!(failed["queuedMessages"][0]["id"], queued_id);
        let full = panel_reply(
            &mut host,
            &rx,
            "queue-full",
            "conversationQueueMessageRead",
            json!({"conversationId":id,"messageId":queued_id}),
        );
        assert_eq!(
            full["result"]["queuedMessage"]["text"],
            "queued-after-failure"
        );
        let replacement = uuid::Uuid::new_v4().to_string();
        let edited = panel_reply(
            &mut host,
            &rx,
            "queue-edit",
            "conversationQueueEdit",
            json!({"conversationId":id,"messageId":queued_id,"expectedText":"queued-after-failure","replacementId":replacement,"text":"edited-after-failure"}),
        );
        assert_eq!(
            edited["result"]["queueChange"]["replacementId"],
            replacement
        );
        let stale = panel_reply(
            &mut host,
            &rx,
            "queue-stale",
            "conversationQueueRemove",
            json!({"conversationId":id,"messageId":queued_id,"expectedText":"queued-after-failure"}),
        );
        assert!(stale["error"].is_string());
        let discard = uuid::Uuid::new_v4().to_string();
        panel_reply(
            &mut host,
            &rx,
            "queue-discard",
            "conversationSend",
            json!({"conversationId":id,"messageId":discard,"text":"discard-before-running"}),
        );
        let removed = panel_reply(
            &mut host,
            &rx,
            "queue-remove",
            "conversationQueueRemove",
            json!({"conversationId":id,"messageId":discard,"expectedText":"discard-before-running"}),
        );
        assert_eq!(removed["result"]["queueChange"]["kind"], "removed");
        let before_resume = conversation_poll(&mut host, &rx, &id, "failed");
        assert_eq!(before_resume["queuedMessages"].as_array().unwrap().len(), 1);
        assert_eq!(
            before_resume["queuedMessages"][0]["text"],
            "edited-after-failure"
        );
        panel_reply(
            &mut host,
            &rx,
            "resume-after-fail",
            "conversationResumeQueue",
            json!({"conversationId":id}),
        );
        let resumed = conversation_poll(&mut host, &rx, &id, "idle");
        assert_eq!(resumed["conversation"]["queued"], 0);
        let output: String = resumed["journal"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "textDelta")
            .filter_map(|event| event["data"]["text"].as_str())
            .collect();
        assert!(output.contains("synthetic reply: edited-after-failure"));
        assert!(!output.contains("synthetic reply: discard-before-running"));
        let busy_id = uuid::Uuid::new_v4().to_string();
        panel_reply(
            &mut host,
            &rx,
            "direct-wait",
            "conversationSend",
            json!({"conversationId":id,"messageId":busy_id,"text":"wait"}),
        );
        conversation_poll(&mut host, &rx, &id, "running");
        let ordinary = uuid::Uuid::new_v4().to_string();
        panel_reply(
            &mut host,
            &rx,
            "ordinary-before-direct",
            "conversationSend",
            json!({"conversationId":id,"messageId":ordinary,"text":"ordinary-after-direct"}),
        );
        let direct = uuid::Uuid::new_v4().to_string();
        let ack = panel_reply(
            &mut host,
            &rx,
            "direct-send",
            "conversationSendDirect",
            json!({"conversationId":id,"messageId":direct,"text":"direct-after-stop"}),
        );
        assert_eq!(ack["result"]["message"]["accepted"], true);
        let after_direct = conversation_poll(&mut host, &rx, &id, "idle");
        assert_eq!(after_direct["conversation"]["queuePaused"], true);
        assert_eq!(after_direct["queuedMessages"][0]["id"], ordinary);
        let events = after_direct["journal"]["events"].as_array().unwrap();
        let stopped_at = events
            .iter()
            .position(|event| event["kind"] == "terminal" && event["data"]["messageId"] == busy_id)
            .unwrap();
        let direct_output = events
            .iter()
            .position(|event| event["kind"] == "textDelta" && event["data"]["messageId"] == direct)
            .unwrap();
        assert!(
            stopped_at < direct_output,
            "Direct text executed before confirmed interruption"
        );
        let repeat = panel_reply(
            &mut host,
            &rx,
            "direct-repeat",
            "conversationSendDirect",
            json!({"conversationId":id,"messageId":direct,"text":"direct-after-stop"}),
        );
        assert_eq!(repeat["result"]["message"]["accepted"], false);
        panel_reply(
            &mut host,
            &rx,
            "direct-resume-ordinary",
            "conversationResumeQueue",
            json!({"conversationId":id}),
        );
        let resumed = conversation_poll(&mut host, &rx, &id, "idle");
        let ready_pids: std::collections::HashSet<_> = resumed["journal"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "ready")
            .map(|e| e["data"]["pid"].as_u64().unwrap())
            .collect();
        assert_eq!(
            ready_pids.len(),
            1,
            "CLI process was restarted between turns"
        );
    }
    for agent in ["codex", "claude"] {
        let created = panel_reply(
            &mut host,
            &rx,
            "exit-direct-create",
            "conversationCreate",
            json!({"agentId":agent,"executable":fixture}),
        );
        let id = created["result"]["conversation"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        cleanup.chats.push(id.clone());
        panel_reply(
            &mut host,
            &rx,
            "exit-direct-wait",
            "conversationSend",
            json!({"conversationId":id,"messageId":uuid::Uuid::new_v4().to_string(),"text":"wait-exit"}),
        );
        conversation_poll(&mut host, &rx, &id, "running");
        let direct = uuid::Uuid::new_v4().to_string();
        panel_reply(
            &mut host,
            &rx,
            "exit-direct-send",
            "conversationSendDirect",
            json!({"conversationId":id,"messageId":direct,"text":"direct-after-process-exit"}),
        );
        let done = conversation_poll(&mut host, &rx, &id, "idle");
        let events = done["journal"]["events"].as_array().unwrap();
        let exited = events
            .iter()
            .position(|event| {
                event["kind"] == "exited" && event["data"]["processExitConfirmed"] == true
            })
            .unwrap();
        let output = events
            .iter()
            .position(|event| event["kind"] == "textDelta" && event["data"]["messageId"] == direct)
            .unwrap();
        assert!(
            exited < output,
            "New process sent direct text before confirmed old-process exit"
        );
        let pids: std::collections::HashSet<_> = events
            .iter()
            .filter(|event| event["kind"] == "ready")
            .map(|event| event["data"]["pid"].as_u64().unwrap())
            .collect();
        assert_eq!(
            pids.len(),
            2,
            "Confirmed exit did not start a fresh CLI process"
        );
    }
    let invalid = panel_reply(
        &mut host,
        &rx,
        "direct-invalid-create",
        "conversationCreate",
        json!({"agentId":"codex","executable":registry.path().join("missing-synthetic-cli")}),
    );
    let invalid_id = invalid["result"]["conversation"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    cleanup.chats.push(invalid_id.clone());
    let invalid_message = uuid::Uuid::new_v4().to_string();
    let not_started = panel_reply(
        &mut host,
        &rx,
        "direct-invalid-send",
        "conversationSendDirect",
        json!({"conversationId":invalid_id,"messageId":invalid_message,"text":"retain this unconfirmed direct message"}),
    );
    assert_eq!(
        not_started["result"]["conversation"]["phase"],
        "recoveryRequired"
    );
    assert_eq!(
        not_started["result"]["conversation"]["activeMessageId"],
        invalid_message
    );
    let refuse_resume = panel_reply(
        &mut host,
        &rx,
        "direct-invalid-resume",
        "conversationResumeQueue",
        json!({"conversationId":invalid_id}),
    );
    assert!(refuse_resume["error"]
        .as_str()
        .unwrap()
        .contains("unconfirmed"));
    let other_profile = "native-other-profile-fixture";
    let _other_cleanup = ServiceFixture {
        root: registry.path().into(),
        profile: other_profile.into(),
        chats: vec![],
    };
    let mut other_host = spawn();
    let other_rx = frames(&mut other_host);
    send(
        &mut other_host,
        json!({"method":"hello","version":"fixture","profileId":other_profile}),
    );
    let forbidden = panel_reply(
        &mut other_host,
        &other_rx,
        "cross-profile",
        "conversationRead",
        json!({"conversationId":cleanup.chats[0],"after":0}),
    );
    assert!(forbidden["error"]
        .as_str()
        .unwrap()
        .contains("this profile"));
    let other_health = panel_reply(
        &mut other_host,
        &other_rx,
        "other-health",
        "serviceHealth",
        json!({}),
    );
    assert_ne!(other_health["result"]["pid"], service_pid);
    let endpoint_path = registry
        .path()
        .join(format!("agent-panel/profiles/{profile}/service.json"));
    let endpoint_data: Value =
        serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();
    let mut unauthenticated =
        std::net::TcpStream::connect(endpoint_data["address"].as_str().unwrap()).unwrap();
    unauthenticated
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let raw=json!({"capability":"synthetic-wrong-capability","requestId":"denied","operation":"health","params":{}}).to_string().into_bytes();
    unauthenticated
        .write_all(&(raw.len() as u32).to_le_bytes())
        .unwrap();
    unauthenticated.write_all(&raw).unwrap();
    let mut byte = [0u8; 1];
    assert_eq!(unauthenticated.read(&mut byte).unwrap(), 0);
    let history_before_restart = conversation_poll(&mut host, &rx, &cleanup.chats[0], "idle");
    let users_before_restart: Vec<Value> = history_before_restart["journal"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "user")
        .map(|event| event["data"].clone())
        .collect();
    let stopped = panel_reply(&mut host, &rx, "service-stop", "serviceStop", json!({}));
    assert_eq!(stopped["result"]["service"]["stopRequested"], true);
    let endpoint = registry
        .path()
        .join(format!("agent-panel/profiles/{profile}/service.json"));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while endpoint.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "Service shutdown was not confirmed"
        );
        std::thread::sleep(Duration::from_millis(40));
    }
    // Restart the independent service without resubmitting any prior intent.
    let restarted = panel_reply(
        &mut host,
        &rx,
        "service-restart",
        "serviceHealth",
        json!({}),
    );
    assert!(restarted.get("error").is_none());
    assert_ne!(restarted["result"]["pid"], service_pid);
    let id = &cleanup.chats[0];
    let before = conversation_poll(&mut host, &rx, id, "idle");
    let count = before["journal"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "user")
        .count();
    let users_after_restart: Vec<Value> = before["journal"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "user")
        .map(|event| event["data"].clone())
        .collect();
    assert_eq!(
        users_after_restart, users_before_restart,
        "Service restart changed prior message identity or text"
    );
    panel_reply(
        &mut host,
        &rx,
        "after-restart",
        "conversationSend",
        json!({"conversationId":id,"messageId":uuid::Uuid::new_v4().to_string(),"text":"new-after-restart"}),
    );
    let after = conversation_poll(&mut host, &rx, id, "idle");
    assert_eq!(
        after["journal"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "user")
            .count(),
        count + 1
    );
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
