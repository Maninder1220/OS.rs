// =============================================================================
// File: src/actions.rs
// Purpose:
//   Guarded action proposal, approval, and execution store for operator-safe remediation commands.
//
// Where this fits in OSAI:
//   Backs the dashboard action workflow and protects command execution behind explicit approval.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   This is security-sensitive. Do not broaden executable commands without reviewing guardrails.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    ReadOnly,
    Repair,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Proposed,
    Approved,
    Rejected,
    Running,
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub reason: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub kind: Option<ActionKind>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub reason: String,
    pub command: String,
    pub args: Vec<String>,
    pub kind: ActionKind,
    pub status: ActionStatus,
    pub requires_approval: bool,
    pub validation_message: String,
    pub approved_at: Option<String>,
    pub output: Option<ActionOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug)]
pub struct ActionStore {
    actions: Mutex<BTreeMap<String, ActionRecord>>,
    audit_path: PathBuf,
}

impl ActionStore {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref();
        fs::create_dir_all(data_dir)?;
        let audit_path = data_dir.join("action_audit.jsonl");
        if !audit_path.exists() {
            fs::File::create(&audit_path)?;
        }

        Ok(Self {
            actions: Mutex::new(BTreeMap::new()),
            audit_path,
        })
    }

    pub fn propose(&self, request: ActionRequest) -> anyhow::Result<ActionRecord> {
        let validation = validate_action(&request.command, &request.args);
        let now = Utc::now().to_rfc3339();
        let kind = request.kind.unwrap_or_else(|| infer_kind(&request.command, &request.args));
        let requires_approval = kind == ActionKind::Repair;

        // Proposal is the safety gate. Read-only allowlisted checks can move to
        // Approved; repair-like actions wait for explicit approval; blocked
        // commands never become runnable.
        let status = if validation.allowed {
            if requires_approval {
                ActionStatus::Proposed
            } else {
                ActionStatus::Approved
            }
        } else {
            ActionStatus::Blocked
        };

        let record = ActionRecord {
            id: next_action_id(),
            created_at: now.clone(),
            updated_at: now,
            reason: request.reason,
            command: request.command,
            args: request.args,
            kind,
            status,
            requires_approval,
            validation_message: validation.message,
            approved_at: None,
            output: None,
        };

        self.actions
            .lock()
            .expect("action lock poisoned")
            .insert(record.id.clone(), record.clone());
        self.audit(&record)?;
        Ok(record)
    }

    pub fn list(&self) -> Vec<ActionRecord> {
        self.actions
            .lock()
            .expect("action lock poisoned")
            .values()
            .rev()
            .cloned()
            .collect()
    }

    pub fn approve(&self, id: &str) -> anyhow::Result<Option<ActionRecord>> {
        let mut guard = self.actions.lock().expect("action lock poisoned");
        let Some(record) = guard.get_mut(id) else {
            return Ok(None);
        };

        if record.status == ActionStatus::Blocked {
            return Ok(Some(record.clone()));
        }

        let now = Utc::now().to_rfc3339();
        record.status = ActionStatus::Approved;
        record.approved_at = Some(now.clone());
        record.updated_at = now;
        let cloned = record.clone();
        drop(guard);
        self.audit(&cloned)?;
        Ok(Some(cloned))
    }

    pub async fn run(&self, id: &str) -> anyhow::Result<Option<ActionRecord>> {
        let record = {
            let mut guard = self.actions.lock().expect("action lock poisoned");
            let Some(record) = guard.get_mut(id) else {
                return Ok(None);
            };

            if record.status != ActionStatus::Approved {
                return Ok(Some(record.clone()));
            }

            record.status = ActionStatus::Running;
            record.updated_at = Utc::now().to_rfc3339();
            record.clone()
        };

        self.audit(&record)?;

        let timeout_seconds = 20;
        let output = execute_command(&record.command, &record.args, timeout_seconds).await;

        let mut final_record = record.clone();
        final_record.updated_at = Utc::now().to_rfc3339();
        final_record.output = Some(output);
        final_record.status = if final_record
            .output
            .as_ref()
            .map(|o| o.exit_code == Some(0) && !o.timed_out)
            .unwrap_or(false)
        {
            ActionStatus::Completed
        } else {
            ActionStatus::Failed
        };

        self.actions
            .lock()
            .expect("action lock poisoned")
            .insert(id.to_string(), final_record.clone());
        self.audit(&final_record)?;

        Ok(Some(final_record))
    }

    fn audit(&self, record: &ActionRecord) -> anyhow::Result<()> {
        let json = serde_json::to_string(record)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_path)?;
        writeln!(file, "{json}")?;
        Ok(())
    }
}

struct ValidationResult {
    allowed: bool,
    message: String,
}

fn validate_action(command: &str, args: &[String]) -> ValidationResult {
    let blocked_programs = ["rm", "mkfs", "dd", "shred", "wipefs", "reboot", "shutdown", "bash", "sh", "sudo"];
    if blocked_programs.contains(&command) {
        return ValidationResult {
            allowed: false,
            message: format!("blocked command: {command}"),
        };
    }

    if command.contains('/') || command.contains(' ') {
        return ValidationResult {
            allowed: false,
            message: "command must be a program name, not a raw shell string".to_string(),
        };
    }

    for arg in args {
        if contains_shell_metacharacter(arg) {
            return ValidationResult {
                allowed: false,
                message: format!("blocked shell metacharacter in argument: {arg}"),
            };
        }
    }

    // Keep this allowlist intentionally small. OSAI should prefer diagnosis and
    // only add new commands after they have a clear read-only or approved use.
    let allowed = match command {
        "df" | "free" | "ss" | "ps" | "du" => true,
        "systemctl" => matches_first(args, &["status", "is-active", "is-enabled", "restart"]),
        "journalctl" => args.iter().any(|a| a == "-u"),
        "kubectl" => matches_first(args, &["get", "describe", "logs"]),
        "gitlab-ctl" => matches_first(args, &["status"]),
        _ => false,
    };

    ValidationResult {
        allowed,
        message: if allowed {
            "command passed allowlist validation".to_string()
        } else {
            "command is not in the OSAI allowlist".to_string()
        },
    }
}

fn infer_kind(command: &str, args: &[String]) -> ActionKind {
    if command == "systemctl" && args.first().map(|x| x.as_str()) == Some("restart") {
        ActionKind::Repair
    } else {
        ActionKind::ReadOnly
    }
}

fn matches_first(args: &[String], allowed_first_args: &[&str]) -> bool {
    args.first()
        .map(|first| allowed_first_args.contains(&first.as_str()))
        .unwrap_or(false)
}

fn contains_shell_metacharacter(value: &str) -> bool {
    [';', '|', '&', '`', '$', '>', '<', '\n', '\r']
        .iter()
        .any(|c| value.contains(*c))
}

async fn execute_command(command: &str, args: &[String], timeout_seconds: u64) -> ActionOutput {
    let mut child = Command::new(command);
    child.args(args);

    let result = tokio::time::timeout(Duration::from_secs(timeout_seconds), child.output()).await;
    match result {
        Ok(Ok(output)) => ActionOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            timed_out: false,
        },
        Ok(Err(err)) => ActionOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: err.to_string(),
            timed_out: false,
        },
        Err(_) => ActionOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: format!("command timed out after {timeout_seconds}s"),
            timed_out: true,
        },
    }
}

fn next_action_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("act-{nanos}")
}
