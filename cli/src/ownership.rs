//! Session ownership + human handoff.
//!
//! Ported from ego-lite's Space ownership model (issue #89), adapted to
//! chrome-use's real-Chrome, sidecar-file session model. A session is owned by
//! the **agent** by default. `session handoff` marks it **user**-owned — the
//! human is now driving (login / 2FA / captcha) — and until `session resume`
//! the agent is refused at the [`crate::connection::send_command`] chokepoint,
//! so it cannot fight the user for the tab. This is zero-impact unless a handoff
//! is explicitly performed: no `.owner` sidecar ⇒ agent-owned ⇒ no guard.
//!
//! Ownership lives in a `<session>.owner` sidecar next to `<session>.pid` /
//! `.sock` / `.version` in [`crate::connection::get_socket_dir`]. Absent (or
//! containing `agent`) ⇒ agent; `user` ⇒ handed off.

use crate::connection::get_socket_dir;
use std::path::PathBuf;

/// Who currently controls a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The agent controls the session (default).
    Agent,
    /// Handed off to the human; the agent must not drive it.
    User,
}

impl Owner {
    pub fn as_str(&self) -> &'static str {
        match self {
            Owner::Agent => "agent",
            Owner::User => "user",
        }
    }

    fn parse(s: &str) -> Owner {
        match s.trim().to_lowercase().as_str() {
            "user" => Owner::User,
            _ => Owner::Agent,
        }
    }
}

/// Path of a session's `.owner` sidecar.
pub fn owner_path(session: &str) -> PathBuf {
    get_socket_dir().join(format!("{session}.owner"))
}

/// The session's current owner. A missing / unreadable / non-`user` sidecar is
/// `Agent` — ownership defaults to the agent, so nothing changes for anyone who
/// never hands off.
pub fn owner_of(session: &str) -> Owner {
    match std::fs::read_to_string(owner_path(session)) {
        Ok(s) => Owner::parse(&s),
        Err(_) => Owner::Agent,
    }
}

/// Hand the session to the user: write the `.owner` sidecar as `user`.
pub fn hand_off(session: &str) -> Result<(), String> {
    let dir = get_socket_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(owner_path(session), "user\n").map_err(|e| e.to_string())
}

/// Resume agent control: remove the `.owner` sidecar (absence ⇒ agent).
/// Idempotent — resuming an already-agent-owned session is a no-op success.
pub fn resume(session: &str) -> Result<(), String> {
    match std::fs::remove_file(owner_path(session)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Actions that stay allowed even while a session is handed off — pure
/// lifecycle / introspection that never drives the page.
const HANDOFF_EXEMPT_ACTIONS: &[&str] = &["status", "ping", "version"];

/// The guard applied at the send chokepoint. `Err(msg)` means the agent tried
/// to drive a handed-off session and must wait for `session resume`. Pure and
/// unit-testable: takes the resolved owner + the command's action.
pub fn guard(session: &str, owner: Owner, action: Option<&str>) -> Result<(), String> {
    if owner != Owner::User {
        return Ok(());
    }
    // Envelopes without an action (lifecycle plumbing) and the exempt set pass.
    match action {
        None => Ok(()),
        Some(a) if HANDOFF_EXEMPT_ACTIONS.contains(&a) => Ok(()),
        Some(_) => Err(format!(
            "session '{session}' is handed off to the user — the agent must not drive it. \
             Run `chrome-use session resume{}` once they confirm they're done.",
            session_flag_suffix(session)
        )),
    }
}

/// `""` for the default session, ` --session <name>` otherwise — so the error /
/// hint prints a command the agent can copy verbatim.
pub fn session_flag_suffix(session: &str) -> String {
    if session == "default" || session.is_empty() {
        String::new()
    } else {
        format!(" --session {session}")
    }
}

/// Represents an active agent exclusive lock over a session and the data directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub struct AgentLockInfo {
    #[allow(dead_code)]
    pub pid: u32,
    #[allow(dead_code)]
    pub owner_id: String,
    #[allow(dead_code)]
    pub session: String,
    #[allow(dead_code)]
    pub created_at: u64,
}

#[allow(dead_code)]
pub fn agent_lock_path(session: &str) -> PathBuf {
    get_socket_dir().join(format!("{session}.agent.lock"))
}

#[allow(dead_code)]
pub fn global_folder_lock_path() -> PathBuf {
    get_socket_dir().join("active_agent.lock")
}

#[allow(dead_code)]
pub fn current_process_owner_id() -> String {
    std::env::var("AGENT_BROWSER_OWNER_ID")
        .unwrap_or_else(|_| format!("agent-pid-{}", std::process::id()))
}

/// Check if the session or folder is locked by another live agent.
/// If locked by another live process without a matching owner ID, returns Err.
#[allow(dead_code)]
pub fn verify_agent_access(session: &str) -> Result<(), String> {
    if cfg!(test) || std::env::var("AGENT_BROWSER_ALLOW_CONCURRENT").is_ok() {
        return Ok(());
    }

    let current_pid = std::process::id();
    let current_owner = current_process_owner_id();
    let daemon_pid = crate::connection::read_registered_daemon_pid(session);

    // 1. Check session lock
    let sess_path = agent_lock_path(session);
    if sess_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&sess_path) {
            if let Ok(info) = serde_json::from_str::<AgentLockInfo>(&content) {
                if crate::connection::is_pid_alive(info.pid) {
                    let is_session_daemon = daemon_pid == Some(info.pid);
                    if !is_session_daemon
                        && info.pid != current_pid
                        && info.owner_id != current_owner
                    {
                        return Err(format!(
                            "folder_access_denied: Session '{session}' is locked by active agent (PID {}). Concurrent agent access denied.",
                            info.pid
                        ));
                    }
                } else {
                    let _ = std::fs::remove_file(&sess_path);
                }
            }
        }
    }

    // 2. Check global folder lock if present
    let glob_path = global_folder_lock_path();
    if glob_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&glob_path) {
            if let Ok(info) = serde_json::from_str::<AgentLockInfo>(&content) {
                if crate::connection::is_pid_alive(info.pid) {
                    let is_session_daemon = daemon_pid == Some(info.pid);
                    if !is_session_daemon
                        && info.pid != current_pid
                        && info.owner_id != current_owner
                        && info.session != session
                    {
                        return Err(format!(
                            "folder_access_denied: Chrome-use data folder is locked exclusively by active agent (PID {}, session '{}'). Concurrent agent access denied.",
                            info.pid, info.session
                        ));
                    }
                } else {
                    let _ = std::fs::remove_file(&glob_path);
                }
            }
        }
    }

    Ok(())
}

/// Acquire an exclusive agent lock on the session and data folder.
#[allow(dead_code)]
pub fn acquire_agent_lock(session: &str) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    verify_agent_access(session)?;

    let dir = get_socket_dir();
    let _ = std::fs::create_dir_all(&dir);

    let info = AgentLockInfo {
        pid: std::process::id(),
        owner_id: current_process_owner_id(),
        session: session.to_string(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };

    let encoded = serde_json::to_string(&info).map_err(|e| e.to_string())?;
    std::fs::write(agent_lock_path(session), &encoded).map_err(|e| e.to_string())?;
    let _ = std::fs::write(global_folder_lock_path(), &encoded);

    Ok(())
}

/// Release the agent lock for this session.
#[allow(dead_code)]
pub fn release_agent_lock(session: &str) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    let sess_path = agent_lock_path(session);
    let _ = std::fs::remove_file(sess_path);

    let glob_path = global_folder_lock_path();
    if glob_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&glob_path) {
            if let Ok(info) = serde_json::from_str::<AgentLockInfo>(&content) {
                if info.pid == std::process::id() || info.session == session {
                    let _ = std::fs::remove_file(&glob_path);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_parse_defaults_to_agent() {
        assert_eq!(Owner::parse("user"), Owner::User);
        assert_eq!(Owner::parse("USER\n"), Owner::User);
        assert_eq!(Owner::parse("agent"), Owner::Agent);
        assert_eq!(Owner::parse(""), Owner::Agent);
        assert_eq!(Owner::parse("garbage"), Owner::Agent);
    }

    #[test]
    fn guard_allows_agent_owned_everything() {
        assert!(guard("default", Owner::Agent, Some("click")).is_ok());
        assert!(guard("default", Owner::Agent, Some("open")).is_ok());
        assert!(guard("default", Owner::Agent, None).is_ok());
    }

    #[test]
    fn guard_refuses_driving_on_handed_off_session() {
        for a in ["click", "open", "type", "fill", "eval", "scroll", "close"] {
            assert!(
                guard("default", Owner::User, Some(a)).is_err(),
                "driving action {a} must be refused while handed off"
            );
        }
    }

    #[test]
    fn guard_exempts_lifecycle_and_actionless_envelopes() {
        for a in HANDOFF_EXEMPT_ACTIONS {
            assert!(guard("default", Owner::User, Some(a)).is_ok());
        }
        // A lifecycle envelope with no action must not be blocked.
        assert!(guard("default", Owner::User, None).is_ok());
    }

    #[test]
    fn guard_message_has_copyable_resume_command() {
        let err = guard("work", Owner::User, Some("click")).unwrap_err();
        assert!(err.contains("session resume --session work"), "got: {err}");
        let err = guard("default", Owner::User, Some("click")).unwrap_err();
        assert!(
            err.contains("session resume") && !err.contains("--session"),
            "got: {err}"
        );
    }

    #[test]
    fn session_flag_suffix_default_is_empty() {
        assert_eq!(session_flag_suffix("default"), "");
        assert_eq!(session_flag_suffix(""), "");
        assert_eq!(session_flag_suffix("work"), " --session work");
    }
}
