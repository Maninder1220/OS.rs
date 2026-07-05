// =============================================================================
// File: src/plugins/gitlab.rs
// Purpose:
//   Detects GitLab-related process signals and suggests safe GitLab inspection commands.
//
// Where this fits in OSAI:
//   Adds GitLab awareness to scanner output and rules without requiring direct GitLab API access.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Only collect hints and safe commands; do not run administrative commands automatically.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::{collections::BTreeSet, path::Path};

use crate::collector::models::GitlabHint;

pub fn collect_gitlab_hints(processes: &BTreeSet<String>) -> GitlabHint {
    let mut signals = Vec::new();
    let mut safe_commands = Vec::new();

    for name in ["gitlab", "gitaly", "gitlab-workhorse", "sidekiq", "puma", "gitlab-runsvdir"] {
        if contains_process(processes, name) {
            signals.push(format!("process detected: {name}"));
        }
    }

    for path in ["/etc/gitlab", "/opt/gitlab", "/var/opt/gitlab", "/var/log/gitlab"] {
        if Path::new(path).exists() {
            signals.push(format!("path exists: {path}"));
        }
    }

    if Path::new("/opt/gitlab/bin/gitlab-ctl").exists() || Path::new("/usr/bin/gitlab-ctl").exists() {
        signals.push("gitlab-ctl binary found".to_string());
    }

    if !signals.is_empty() {
        safe_commands.extend([
            "gitlab-ctl status".to_string(),
            "systemctl status gitlab-runsvdir".to_string(),
            "journalctl -u gitlab-runsvdir --since -1h".to_string(),
        ]);
    }

    GitlabHint {
        detected: !signals.is_empty(),
        available: !signals.is_empty(),
        summary: if signals.is_empty() {
            "No GitLab process or path signal was detected.".to_string()
        } else {
            "GitLab plugin detected installation/runtime signals. Previous incident memory says auto-start can cause high CPU/RAM.".to_string()
        },
        signals,
        safe_commands,
    }
}

fn contains_process(processes: &BTreeSet<String>, needle: &str) -> bool {
    processes.iter().any(|name| name.contains(needle))
}
