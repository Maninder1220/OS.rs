// =============================================================================
// File: src/history.rs
// Purpose:
//   Local JSONL history storage for scan records and summary retrieval.
//
// Where this fits in OSAI:
//   Provides lightweight history even before PostgreSQL/RustFS persistence is considered.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   File writes should remain append-friendly and tolerant of partial/corrupt historical lines.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::collector::Snapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub generated_at: String,
    pub hostname: String,
    pub finding_count: usize,
    pub warn_count: usize,
    pub critical_count: usize,
    pub highest_severity: String,
    pub snapshot: Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySummary {
    pub id: String,
    pub generated_at: String,
    pub hostname: String,
    pub finding_count: usize,
    pub warn_count: usize,
    pub critical_count: usize,
    pub highest_severity: String,
}

#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref();
        fs::create_dir_all(data_dir)?;
        let path = data_dir.join("scan_history.jsonl");
        if !path.exists() {
            File::create(&path)?;
        }
        Ok(Self {
            path,
            lock: Mutex::new(()),
        })
    }

    pub fn append_snapshot(&self, snapshot: &Snapshot) -> anyhow::Result<HistoryRecord> {
        let _guard = self.lock.lock().expect("history lock poisoned");
        let record = HistoryRecord::from_snapshot(snapshot.clone());
        let json = serde_json::to_string(&record)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{json}")?;
        Ok(record)
    }

    pub fn list_recent(&self, limit: usize) -> anyhow::Result<Vec<HistorySummary>> {
        let _guard = self.lock.lock().expect("history lock poisoned");
        let mut records = self.read_all_records()?;
        records.reverse();
        records.truncate(limit.max(1));
        Ok(records.into_iter().map(HistorySummary::from).collect())
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Option<HistoryRecord>> {
        let _guard = self.lock.lock().expect("history lock poisoned");
        Ok(self.read_all_records()?.into_iter().find(|r| r.id == id))
    }

    fn read_all_records(&self) -> anyhow::Result<Vec<HistoryRecord>> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<HistoryRecord>(&line) {
                records.push(record);
            }
        }

        Ok(records)
    }
}

impl HistoryRecord {
    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        let warn_count = snapshot
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .count();
        let critical_count = snapshot
            .findings
            .iter()
            .filter(|f| f.severity == "critical")
            .count();
        let highest_severity = if critical_count > 0 {
            "critical"
        } else if warn_count > 0 {
            "warn"
        } else if snapshot.findings.is_empty() {
            "ok"
        } else {
            "info"
        }
        .to_string();

        Self {
            id: next_id(&snapshot.host.hostname),
            generated_at: snapshot.generated_at.clone(),
            hostname: snapshot.host.hostname.clone(),
            finding_count: snapshot.findings.len(),
            warn_count,
            critical_count,
            highest_severity,
            snapshot,
        }
    }
}

impl From<HistoryRecord> for HistorySummary {
    fn from(record: HistoryRecord) -> Self {
        Self {
            id: record.id,
            generated_at: record.generated_at,
            hostname: record.hostname,
            finding_count: record.finding_count,
            warn_count: record.warn_count,
            critical_count: record.critical_count,
            highest_severity: record.highest_severity,
        }
    }
}

fn next_id(hostname: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let safe_host = hostname
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>();
    format!("{}-{nanos}", safe_host.trim_matches('-'))
}
