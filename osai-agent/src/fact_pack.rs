// =============================================================================
// File: src/fact_pack.rs
// Purpose:
//   Builds a compact evidence bundle from an AskPlan and the current OSAI snapshot.
//
// Where this fits in OSAI:
//   Ask OSAI sends this focused FactPack to Cognee/Qwen instead of dumping the whole server state.
//
// Topics to know before editing:
//   OSAI Snapshot fields, intent planning, serde serialization, and operational evidence selection.
//
// Important operational notes:
//   FactPack is a prompt budget control. Add facts only when they directly help answer the detected intent.
// =============================================================================

use serde::Serialize;

use crate::{
    ask::LatestScanContext,
    ask_plan::{AskPlan, Intent},
    collector::Snapshot,
};

#[derive(Debug, Clone, Serialize)]
pub struct FactPack {
    pub title: String,
    pub intents: Vec<Intent>,
    pub facts: Vec<Fact>,
    pub findings: Vec<FocusedFinding>,
    pub metrics: Vec<FocusedMetric>,
    pub manual_checks: Vec<String>,
    pub cognee_query: Option<String>,
    pub safety_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FactPackSummary {
    pub title: String,
    pub intent_count: usize,
    pub fact_count: usize,
    pub finding_count: usize,
    pub metric_count: usize,
    pub manual_check_count: usize,
    pub cognee_query: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
    pub severity: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FocusedFinding {
    pub severity: String,
    pub title: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FocusedMetric {
    pub label: String,
    pub value: String,
    pub unit: String,
    pub percent: Option<f64>,
}

impl FactPack {
    pub fn summary(&self) -> FactPackSummary {
        FactPackSummary {
            title: self.title.clone(),
            intent_count: self.intents.len(),
            fact_count: self.facts.len(),
            finding_count: self.findings.len(),
            metric_count: self.metrics.len(),
            manual_check_count: self.manual_checks.len(),
            cognee_query: self.cognee_query.clone(),
        }
    }
}

pub fn build_fact_pack(plan: &AskPlan, snapshot: &Snapshot, latest: Option<&LatestScanContext>) -> FactPack {
    let mut pack = FactPack {
        title: title_for(&plan.intents),
        intents: plan.intents.clone(),
        facts: Vec::new(),
        findings: Vec::new(),
        metrics: Vec::new(),
        manual_checks: Vec::new(),
        cognee_query: None,
        safety_notes: vec![
            "Rust scanner facts are source of truth.".to_string(),
            "Suggested checks are read-only unless an action is explicitly approved.".to_string(),
        ],
    };

    for intent in &plan.intents {
        match intent {
            Intent::Cpu => add_cpu(&mut pack, snapshot),
            Intent::Memory => add_memory(&mut pack, snapshot),
            Intent::Storage => add_storage(&mut pack, snapshot),
            Intent::NetworkPorts => add_network_ports(&mut pack, snapshot),
            Intent::Processes => add_processes(&mut pack, snapshot),
            Intent::Services => add_services(&mut pack, snapshot),
            Intent::Databases => add_databases(&mut pack, snapshot),
            Intent::Kubernetes => add_kubernetes(&mut pack, snapshot),
            Intent::GitLab => add_gitlab(&mut pack, snapshot),
            Intent::Findings => add_findings(&mut pack, snapshot, plan.fact_budget),
            Intent::Actions => add_actions(&mut pack),
            Intent::ServerOverview => add_overview(&mut pack, snapshot, latest),
        }
    }

    if pack.findings.is_empty() {
        add_findings(&mut pack, snapshot, 4);
    }

    pack.manual_checks.sort();
    pack.manual_checks.dedup();
    pack.facts.truncate(plan.fact_budget);
    pack.metrics.truncate(plan.fact_budget);
    pack.findings.truncate(plan.fact_budget);

    if plan.use_cognee {
        pack.cognee_query = Some(build_cognee_query(plan, snapshot));
    }

    pack
}

fn add_overview(pack: &mut FactPack, snapshot: &Snapshot, latest: Option<&LatestScanContext>) {
    push_fact(pack, "Host", &snapshot.host.hostname, "ok", "Current scanned hostname.");
    push_fact(pack, "OS", &snapshot.os.long_version, "ok", "Current operating system.");
    push_fact(pack, "Findings", &snapshot.findings.len().to_string(), finding_severity(snapshot), "Current rule findings count.");
    if let Some(scan) = latest {
        push_fact(pack, "Latest stored scan", &scan.generated_at, &scan.highest_severity, "Most recent PostgreSQL scan row.");
    }
    pack.manual_checks.extend([
        "hostnamectl".to_string(),
        "uptime".to_string(),
        "free -m".to_string(),
        "df -h".to_string(),
        "systemctl --failed".to_string(),
    ]);
}

fn add_cpu(pack: &mut FactPack, snapshot: &Snapshot) {
    let usage = snapshot.compute.global_cpu_usage_percent as f64;
    push_metric(pack, "Global CPU usage", format!("{usage:.1}"), "%", Some(usage));
    push_fact(pack, "Logical CPUs", &snapshot.compute.logical_cpus.to_string(), "ok", "Total schedulable logical CPU count.");
    let high_cores = snapshot.compute.cpus.iter().filter(|cpu| cpu.usage_percent >= 85.0).count();
    push_fact(pack, "High-use cores", &high_cores.to_string(), severity_for_percent(usage), "Cores at or above 85% CPU.");
    pack.manual_checks.extend([
        "uptime".to_string(),
        "top -o %CPU".to_string(),
        "ps -eo pid,comm,%cpu,%mem --sort=-%cpu | head".to_string(),
    ]);
}

fn add_memory(pack: &mut FactPack, snapshot: &Snapshot) {
    let total = snapshot.memory.total_bytes;
    let used = snapshot.memory.used_bytes;
    let memory_percent = percent_value(used, total);
    push_metric(pack, "Memory used", human_bytes(used), "", memory_percent);
    push_metric(pack, "Memory total", human_bytes(total), "", None);
    push_metric(pack, "Swap used", human_bytes(snapshot.memory.used_swap_bytes), "", percent_value(snapshot.memory.used_swap_bytes, snapshot.memory.total_swap_bytes));
    push_fact(pack, "Available memory", &human_bytes(snapshot.memory.available_bytes), severity_for_percent(memory_percent.unwrap_or(0.0)), "RAM available for new workload without swapping.");
    pack.manual_checks.extend([
        "free -h".to_string(),
        "vmstat 1 5".to_string(),
        "top -o %MEM".to_string(),
    ]);
}

fn add_storage(pack: &mut FactPack, snapshot: &Snapshot) {
    for disk in snapshot.storage.iter().take(5) {
        push_metric(pack, &format!("{} used", disk.mount_point), format!("{:.1}", disk.used_percent), "%", Some(disk.used_percent));
    }
    pack.manual_checks.extend([
        "df -h".to_string(),
        "du -xh / --max-depth=1 2>/dev/null | sort -h".to_string(),
    ]);
}

fn add_network_ports(pack: &mut FactPack, snapshot: &Snapshot) {
    push_fact(pack, "Listening ports", &snapshot.listening_ports.len().to_string(), "info", "Open local listening sockets from the scan.");
    for port in snapshot.listening_ports.iter().take(8) {
        push_fact(pack, &format!("{} {}", port.protocol, port.port), &port.local_address_raw, "info", "Listening socket.");
    }
    pack.manual_checks.extend([
        "ss -tulpen".to_string(),
        "ip addr".to_string(),
        "firewall-cmd --list-all".to_string(),
    ]);
}

fn add_processes(pack: &mut FactPack, snapshot: &Snapshot) {
    for process in snapshot.top_processes.iter().take(6) {
        push_fact(pack, &process.name, &format!("pid={} cpu={:.1}% mem={}", process.pid, process.cpu_usage_percent, human_bytes(process.memory_bytes)), "info", "Top process by memory/CPU.");
    }
    pack.manual_checks.push("ps aux --sort=-%mem | head".to_string());
}

fn add_services(pack: &mut FactPack, snapshot: &Snapshot) {
    for service in snapshot.service_hints.iter().take(8) {
        push_fact(pack, &service.name, &service.confidence, "info", "Detected service hint from process table.");
    }
    pack.manual_checks.extend([
        "systemctl --failed".to_string(),
        "systemctl list-units --type=service --state=running".to_string(),
    ]);
}

fn add_databases(pack: &mut FactPack, snapshot: &Snapshot) {
    for db in snapshot.database_hints.iter().take(8) {
        push_fact(pack, &db.name, &db.confidence, "info", "Detected database hint.");
    }
    pack.manual_checks.extend([
        "ss -tulpen | grep -E '5432|3306|6379|27017'".to_string(),
        "ps aux | grep -E 'postgres|mysql|redis|valkey|mongo'".to_string(),
    ]);
}

fn add_kubernetes(pack: &mut FactPack, snapshot: &Snapshot) {
    push_fact(pack, "Kubernetes detected", &snapshot.kubernetes.detected.to_string(), "info", &snapshot.kubernetes.summary);
    pack.manual_checks.extend(snapshot.kubernetes.safe_commands.iter().cloned());
}

fn add_gitlab(pack: &mut FactPack, snapshot: &Snapshot) {
    push_fact(pack, "GitLab detected", &snapshot.gitlab.detected.to_string(), "info", &snapshot.gitlab.summary);
    pack.manual_checks.extend(snapshot.gitlab.safe_commands.iter().cloned());
}

fn add_findings(pack: &mut FactPack, snapshot: &Snapshot, limit: usize) {
    for finding in snapshot.findings.iter().take(limit) {
        pack.findings.push(FocusedFinding {
            severity: finding.severity.clone(),
            title: finding.title.clone(),
            recommendation: finding.recommendation.clone(),
        });
    }
}

fn add_actions(pack: &mut FactPack) {
    pack.safety_notes.push("Actions require explicit proposal and approval before execution.".to_string());
    pack.manual_checks.push("Review /api/actions before approving any repair command.".to_string());
}

fn build_cognee_query(plan: &AskPlan, snapshot: &Snapshot) -> String {
    let intent_names = plan
        .intents
        .iter()
        .map(|intent| format!("{intent:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "host {} previous incidents repeated patterns resolved issues operational memory for intents: {}",
        snapshot.host.hostname, intent_names
    )
}

fn title_for(intents: &[Intent]) -> String {
    if intents.len() == 1 {
        match intents[0] {
            Intent::Cpu => "CPU status".to_string(),
            Intent::Memory => "Memory status".to_string(),
            Intent::Storage => "Storage status".to_string(),
            Intent::NetworkPorts => "Network and ports status".to_string(),
            Intent::Processes => "Process status".to_string(),
            Intent::Services => "Service status".to_string(),
            Intent::Databases => "Database status".to_string(),
            Intent::Kubernetes => "Kubernetes status".to_string(),
            Intent::GitLab => "GitLab status".to_string(),
            Intent::Findings => "Findings status".to_string(),
            Intent::Actions => "Action safety status".to_string(),
            Intent::ServerOverview => "Server overview".to_string(),
        }
    } else {
        "Focused OSAI status".to_string()
    }
}

fn push_fact(pack: &mut FactPack, label: &str, value: &str, severity: &str, explanation: &str) {
    pack.facts.push(Fact {
        label: label.to_string(),
        value: value.to_string(),
        severity: severity.to_string(),
        explanation: explanation.to_string(),
    });
}

fn push_metric(pack: &mut FactPack, label: &str, value: String, unit: &str, percent: Option<f64>) {
    pack.metrics.push(FocusedMetric {
        label: label.to_string(),
        value,
        unit: unit.to_string(),
        percent,
    });
}

fn finding_severity(snapshot: &Snapshot) -> &'static str {
    if snapshot.findings.iter().any(|finding| finding.severity == "critical") {
        "critical"
    } else if snapshot.findings.iter().any(|finding| finding.severity == "warn") {
        "warn"
    } else {
        "ok"
    }
}

fn severity_for_percent(value: f64) -> &'static str {
    if value >= 90.0 {
        "critical"
    } else if value >= 75.0 {
        "warn"
    } else {
        "ok"
    }
}

fn percent_value(part: u64, total: u64) -> Option<f64> {
    if total == 0 {
        None
    } else {
        Some((part as f64 / total as f64) * 100.0)
    }
}

fn human_bytes(value: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}
