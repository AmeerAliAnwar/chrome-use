//! Durable conversation intents and events. Backend acceptance and completion
//! are separate states; reconnecting never dispatches an existing message.

use super::config_transaction::{atomic_write, private_tree};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const MAX_STATE: u64 = 8 * 1024 * 1024;
const MAX_HISTORY: u64 = 128 * 1024 * 1024;
const MAX_EVENT: usize = 64 * 1024;

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Phase {
    Idle,
    Starting,
    Running,
    WaitingInput,
    Stopping,
    Failed,
    Interrupted,
    RecoveryRequired,
}
impl Phase {
    pub(super) fn busy(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::WaitingInput | Self::Stopping
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Message {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Event {
    pub sequence: u64,
    pub at: u64,
    pub kind: String,
    pub data: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Conversation {
    pub schema_version: u32,
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
    pub phase: Phase,
    pub generation: u64,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub active: Option<Message>,
    pub queue: VecDeque<Message>,
    pub queue_paused: bool,
    pub accepted: BTreeMap<String, String>,
    pub next_sequence: u64,
    pub last_error: Option<String>,
    pub executable: Option<PathBuf>,
}

impl Conversation {
    pub(super) fn new(
        agent_id: &str,
        model: Option<String>,
        executable: Option<PathBuf>,
    ) -> Result<Self, String> {
        if !["codex", "claude"].contains(&agent_id) {
            return Err("This agent has no conversational adapter yet".into());
        }
        if model
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 256 || s.contains(['\r', '\n', '\0']))
        {
            return Err("Invalid conversation model".into());
        }
        let at = now();
        Ok(Self {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            title: "New conversation".into(),
            created: at,
            updated: at,
            phase: Phase::Idle,
            generation: 0,
            session_id: None,
            model: None,
            requested_model: model,
            active: None,
            queue: VecDeque::new(),
            queue_paused: false,
            accepted: BTreeMap::new(),
            next_sequence: 1,
            last_error: None,
            executable,
        })
    }
    pub(super) fn summary(&self) -> Value {
        json!({"id":self.id,"agentId":self.agent_id,"title":self.title,"created":self.created,"updated":self.updated,"phase":self.phase,"generation":self.generation,"model":self.model,"requestedModel":self.requested_model,"queued":self.queue.len(),"queuePaused":self.queue_paused,"activeMessageId":self.active.as_ref().map(|m|&m.id),"lastError":self.last_error,"lastSequence":self.next_sequence.saturating_sub(1),"target":{"mode":"agentSession","session":format!("panel-{}",self.id)}})
    }
    pub(super) fn accept(&mut self, message: Message) -> Result<bool, String> {
        uuid::Uuid::parse_str(&message.id).map_err(|_| "A message needs a stable UUID")?;
        if message.text.trim().is_empty() || message.text.len() > 32 * 1024 {
            return Err("A message must contain between one and 32768 bytes of text".into());
        }
        let hash = hex::encode(Sha256::digest(message.text.as_bytes()));
        if let Some(existing) = self.accepted.get(&message.id) {
            if existing != &hash {
                return Err("A message id was reused for different text".into());
            }
            return Ok(false);
        }
        if self.phase == Phase::RecoveryRequired {
            return Err("The previous turn is unconfirmed; inspect it before continuing".into());
        }
        if self.queue.len() >= 64 || self.accepted.len() >= 50000 {
            return Err("Conversation queue or deduplication capacity reached".into());
        }
        if self.accepted.is_empty() {
            self.title = message
                .text
                .lines()
                .next()
                .unwrap_or("New conversation")
                .chars()
                .take(80)
                .collect();
        }
        self.accepted.insert(message.id.clone(), hash);
        self.queue.push_back(message);
        self.updated = now();
        Ok(true)
    }
    pub(super) fn begin_next(&mut self) -> Option<Message> {
        if self.phase.busy() || self.queue_paused || self.phase == Phase::RecoveryRequired {
            return None;
        }
        let next = self.queue.pop_front()?;
        self.active = Some(next.clone());
        self.phase = Phase::Starting;
        self.last_error = None;
        Some(next)
    }
    pub(super) fn terminal(
        &mut self,
        generation: u64,
        outcome: &str,
        error: Option<String>,
    ) -> Result<bool, String> {
        if generation != self.generation || self.active.is_none() {
            return Ok(false);
        }
        self.phase = match outcome {
            "completed" => Phase::Idle,
            "interrupted" => Phase::Interrupted,
            "failed" => Phase::Failed,
            _ => return Err("Unknown backend terminal status".into()),
        };
        self.queue_paused = outcome != "completed";
        self.last_error = error;
        self.active = None;
        self.updated = now();
        Ok(true)
    }
}

pub(super) struct Store {
    pub directory: PathBuf,
    index: BTreeMap<String, Value>,
}
impl Store {
    pub(super) fn open(directory: PathBuf) -> Result<Self, String> {
        private_tree(&directory)?;
        let path = directory.join("index.json");
        let mut index: BTreeMap<String, Value> = if path.exists() {
            let data = read_private(&path, MAX_STATE)?;
            serde_json::from_slice(&data)
                .map_err(|_| "Conversation index is corrupt; existing history was preserved")?
        } else {
            BTreeMap::new()
        };
        for value in index.values_mut() {
            if ["starting", "running", "waitingInput", "stopping"]
                .contains(&value["phase"].as_str().unwrap_or(""))
            {
                value["phase"] = "recoveryRequired".into();
                value["queuePaused"] = true.into();
                value["lastError"] =
                    "The previous service exited; active turn outcome is unconfirmed".into();
            }
        }
        Ok(Self { directory, index })
    }
    fn folder(&self, id: &str) -> Result<PathBuf, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid conversation id")?;
        Ok(self.directory.join(id))
    }
    pub(super) fn list(&self) -> Vec<Value> {
        let mut rows: Vec<_> = self.index.values().cloned().collect();
        rows.sort_by_key(|v| std::cmp::Reverse(v["updated"].as_u64().unwrap_or(0)));
        rows
    }
    pub(super) fn create(&mut self, chat: &Conversation) -> Result<(), String> {
        if self.index.len() >= 10000 {
            return Err(
                "Conversation index capacity reached; archive or delete history first".into(),
            );
        }
        let folder = self.folder(&chat.id)?;
        if folder.exists() {
            return Err("Conversation id already exists".into());
        }
        private_tree(&folder)?;
        self.save(chat)
    }
    pub(super) fn load(&mut self, id: &str) -> Result<Conversation, String> {
        if !self.index.contains_key(id) {
            return Err("Conversation was not found in this profile".into());
        }
        let raw = read_private(&self.folder(id)?.join("state.json"), MAX_STATE)?;
        let mut chat: Conversation = serde_json::from_slice(&raw)
            .map_err(|_| "Conversation state is corrupt; it was not replaced")?;
        if chat.schema_version != 1 || chat.id != id {
            return Err("Conversation identity does not match its state".into());
        }
        let mut cursor = 0;
        loop {
            let page = self.events(id, cursor)?;
            cursor = page["cursor"].as_u64().ok_or("Invalid event cursor")?;
            if page["more"] != true {
                break;
            }
        }
        if chat.next_sequence > cursor + 1 {
            return Err(
                "Conversation state is ahead of its journal; history cannot be assumed complete"
                    .into(),
            );
        }
        chat.next_sequence = cursor + 1;
        if chat.phase.busy() {
            chat.phase = Phase::RecoveryRequired;
            chat.queue_paused = true;
            chat.last_error=Some("The service lost its previous process; turn outcome is unconfirmed and no message was replayed".into());
            self.save(&chat)?;
        }
        Ok(chat)
    }
    pub(super) fn save(&mut self, chat: &Conversation) -> Result<(), String> {
        let bytes = serde_json::to_vec(chat).map_err(|_| "Cannot encode conversation state")?;
        if bytes.len() as u64 > MAX_STATE {
            return Err("Conversation state capacity reached".into());
        }
        atomic_write(&self.folder(&chat.id)?.join("state.json"), &bytes, None)?;
        self.index.insert(chat.id.clone(), chat.summary());
        atomic_write(
            &self.directory.join("index.json"),
            &serde_json::to_vec(&self.index).map_err(|_| "Cannot encode conversation index")?,
            None,
        )
    }
    pub(super) fn event(
        &mut self,
        chat: &mut Conversation,
        kind: &str,
        data: Value,
    ) -> Result<Event, String> {
        let event = Event {
            sequence: chat.next_sequence,
            at: now(),
            kind: kind.into(),
            data,
        };
        let mut raw = serde_json::to_vec(&event).map_err(|_| "Cannot encode conversation event")?;
        if raw.len() > MAX_EVENT {
            return Err("Conversation event exceeds its size limit".into());
        }
        raw.push(b'\n');
        let path = self.folder(&chat.id)?.join("events.jsonl");
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.is_file()
                || meta.file_type().is_symlink()
                || meta.len() + raw.len() as u64 > MAX_HISTORY
            {
                return Err("Conversation history cannot accept this event".into());
            }
        }
        let mut options = OpenOptions::new();
        options.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options
            .open(path)
            .map_err(|_| "Cannot open conversation journal")?;
        file.write_all(&raw)
            .map_err(|_| "Cannot append conversation event")?;
        if kind != "textDelta" {
            file.sync_data()
                .map_err(|_| "Cannot flush conversation event")?;
        }
        chat.next_sequence += 1;
        chat.updated = event.at;
        Ok(event)
    }
    pub(super) fn events(&self, id: &str, after: u64) -> Result<Value, String> {
        if !self.index.contains_key(id) {
            return Err("Conversation was not found in this profile".into());
        }
        let path = self.folder(id)?.join("events.jsonl");
        if !path.exists() {
            return Ok(json!({"events":[],"cursor":after,"more":false}));
        }
        let raw = read_private(&path, MAX_HISTORY)?;
        let mut rows = Vec::new();
        let mut bytes = 0;
        let mut cursor = after;
        let mut more = false;
        let mut previous = 0;
        for line in BufReader::new(raw.as_slice()).split(b'\n') {
            let line = line.map_err(|_| "Cannot read conversation history")?;
            if line.is_empty() {
                continue;
            }
            let event:Event=serde_json::from_slice(&line).map_err(|_|"Conversation journal is incomplete or corrupt; do not assume missing events succeeded")?;
            if event.sequence != previous + 1 {
                return Err("Conversation event sequence has a gap or duplicate".into());
            }
            previous = event.sequence;
            if event.sequence <= after {
                continue;
            }
            if rows.len() >= 128 || bytes + line.len() > 512 * 1024 {
                more = true;
                break;
            }
            bytes += line.len();
            cursor = event.sequence;
            rows.push(event);
        }
        Ok(json!({"events":rows,"cursor":cursor,"more":more}))
    }
}

fn read_private(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let meta = fs::symlink_metadata(path).map_err(|_| "Cannot inspect conversation file")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max {
        return Err("Conversation file is not a bounded regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut data = Vec::new();
    options
        .open(path)
        .map_err(|_| "Cannot open conversation file")?
        .take(max + 1)
        .read_to_end(&mut data)
        .map_err(|_| "Cannot read conversation file")?;
    if data.len() as u64 > max {
        return Err("Conversation file exceeds its size limit".into());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(text: &str) -> Message {
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.into(),
        }
    }
    #[test]
    fn duplicate_send_is_idempotent_but_reusing_an_id_for_other_text_is_refused() {
        let mut c = Conversation::new("codex", None, None).unwrap();
        let m = message("one");
        assert!(c.accept(m.clone()).unwrap());
        assert!(!c.accept(m.clone()).unwrap());
        let mut different = m;
        different.text = "two".into();
        assert!(c.accept(different).is_err());
        assert_eq!(c.queue.len(), 1);
    }
    #[test]
    fn failure_pauses_queued_messages_and_obsolete_processes_cannot_finish_a_turn() {
        let mut c = Conversation::new("codex", None, None).unwrap();
        c.accept(message("one")).unwrap();
        c.accept(message("two")).unwrap();
        c.begin_next().unwrap();
        c.generation = 2;
        assert!(!c.terminal(1, "completed", None).unwrap());
        assert!(c
            .terminal(2, "failed", Some("fixture failure".into()))
            .unwrap());
        assert!(c.begin_next().is_none());
        assert_eq!(c.queue.len(), 1);
    }
    #[test]
    fn unknown_terminal_status_is_not_success() {
        let mut c = Conversation::new("claude", None, None).unwrap();
        c.accept(message("one")).unwrap();
        c.begin_next();
        assert!(c.terminal(0, "mystery", None).is_err());
        assert!(c.phase.busy());
    }
    #[test]
    fn restart_preserves_history_and_marks_running_work_unconfirmed_without_resubmitting() {
        let t = tempfile::tempdir().unwrap();
        let mut s = Store::open(t.path().join("chats")).unwrap();
        let mut c = Conversation::new("codex", None, None).unwrap();
        s.create(&c).unwrap();
        c.accept(message("one")).unwrap();
        c.begin_next();
        s.event(&mut c, "user", json!({"text":"one"})).unwrap();
        s.save(&c).unwrap();
        let mut reopened = Store::open(t.path().join("chats")).unwrap();
        let recovered = reopened.load(&c.id).unwrap();
        assert_eq!(recovered.phase, Phase::RecoveryRequired);
        assert!(recovered.queue_paused);
        assert_eq!(
            reopened.events(&c.id, 0).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn event_replay_is_cursor_based_and_corrupt_history_never_becomes_an_empty_success() {
        let t = tempfile::tempdir().unwrap();
        let mut s = Store::open(t.path().join("chats")).unwrap();
        let mut c = Conversation::new("codex", None, None).unwrap();
        s.create(&c).unwrap();
        s.event(&mut c, "user", json!({"text":"one"})).unwrap();
        s.event(&mut c, "text", json!({"text":"two"})).unwrap();
        s.save(&c).unwrap();
        assert_eq!(
            s.events(&c.id, 1).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let p = s.folder(&c.id).unwrap().join("events.jsonl");
        OpenOptions::new()
            .append(true)
            .open(p)
            .unwrap()
            .write_all(b"{broken")
            .unwrap();
        assert!(s.events(&c.id, 0).is_err());
    }
}
