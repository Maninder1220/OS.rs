// =============================================================================
// File: src/ask_plan.rs
// Purpose:
//   Converts a natural operator question into a focused AskPlan before Qwen sees anything.
//
// Where this fits in OSAI:
//   Ask OSAI uses this planner so Rust chooses the relevant facts, Cognee recall policy, and answer budget first.
//
// Topics to know before editing:
//   Rust enums/structs, serde serialization, simple NLP keyword matching, and OSAI scanner data boundaries.
//
// Important operational notes:
//   Focused intent wins over broad overview. Qwen should refine a plan, not decide what host data to inspect.
// =============================================================================

use serde::Serialize;

use crate::collector::{models::Finding, Snapshot};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Intent {
    ServerOverview,
    Cpu,
    Memory,
    Storage,
    NetworkPorts,
    Processes,
    Services,
    Databases,
    Kubernetes,
    GitLab,
    Findings,
    Actions,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ResponseStyle {
    Conversational,
    Incident,
    Checklist,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum AnswerDepth {
    Short,
    Normal,
    Deep,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskPlan {
    pub original_question: String,
    pub normalized_terms: Vec<String>,
    pub intents: Vec<Intent>,
    pub response_style: ResponseStyle,
    pub depth: AnswerDepth,
    pub use_cognee: bool,
    pub fact_budget: usize,
    pub llm_max_tokens: u64,
    pub planning_note: String,
}

pub fn plan_question(question: &str, snapshot: &Snapshot) -> AskPlan {
    let terms = normalize_terms(question);
    let mut intents = Vec::new();

    push_if(&mut intents, Intent::Cpu, has_any(&terms, &["cpu", "core", "processor", "load"]));
    push_if(&mut intents, Intent::Memory, has_any(&terms, &["ram", "memory", "swap"]));
    push_if(&mut intents, Intent::Storage, has_any(&terms, &["disk", "storage", "filesystem", "mount", "space"]));
    push_if(&mut intents, Intent::Services, has_any(&terms, &["service", "daemon", "systemd"]));
    push_if(&mut intents, Intent::Processes, has_any(&terms, &["process", "pid", "top", "app"]));
    push_if(&mut intents, Intent::Databases, has_any(&terms, &["database", "db", "postgres", "postgresql", "mysql", "redis", "valkey", "mongo"]));
    push_if(&mut intents, Intent::NetworkPorts, has_any(&terms, &["network", "port", "listening", "socket", "firewall"]));
    push_if(&mut intents, Intent::Findings, has_any(&terms, &["issue", "warning", "critical", "finding", "problem", "alert", "error", "failed"]));
    push_if(&mut intents, Intent::Kubernetes, has_any(&terms, &["kubernetes", "k8s", "kubectl", "pod", "node"]));
    push_if(&mut intents, Intent::GitLab, has_any(&terms, &["gitlab", "gitaly", "workhorse"]));
    push_if(&mut intents, Intent::Actions, has_any(&terms, &["action", "fix", "repair", "approve", "run", "command"]));

    let focused = !intents.is_empty();
    let asks_overview = has_any(&terms, &["update", "overview", "status", "health", "server", "host", "machine", "system"]);
    if !focused && asks_overview {
        intents.push(Intent::ServerOverview);
    }
    if intents.is_empty() {
        intents.push(Intent::ServerOverview);
    }

    let historical_words = has_any(&terms, &["before", "previous", "past", "again", "repeat", "repeated", "incident", "history", "resolved", "pattern"]);
    let focused_risk = has_relevant_warning_or_critical(&intents, snapshot);
    let memory_intents = intents.iter().any(|intent| matches!(
        intent,
        Intent::GitLab | Intent::Kubernetes | Intent::Services | Intent::Findings | Intent::Actions
    ));
    let use_cognee = historical_words || focused_risk || memory_intents;

    let depth = if has_any(&terms, &["deep", "detail", "details", "explain", "why", "full"] ) {
        AnswerDepth::Deep
    } else if focused {
        AnswerDepth::Short
    } else {
        AnswerDepth::Normal
    };

    let response_style = if focused_risk || intents.contains(&Intent::Findings) {
        ResponseStyle::Incident
    } else if intents.contains(&Intent::Actions) {
        ResponseStyle::Checklist
    } else {
        ResponseStyle::Conversational
    };

    let llm_max_tokens = match depth {
        AnswerDepth::Short => 120,
        AnswerDepth::Normal => 220,
        AnswerDepth::Deep => 360,
    };

    AskPlan {
        original_question: question.to_string(),
        normalized_terms: terms,
        intents,
        response_style,
        depth,
        use_cognee,
        fact_budget: if focused { 8 } else { 14 },
        llm_max_tokens,
        planning_note: if focused {
            "Focused intent detected; Qwen receives only the matching FactPack and optional memory.".to_string()
        } else {
            "No focused intent detected; Qwen receives a compact server-overview FactPack.".to_string()
        },
    }
}

pub fn plan_needs_latest_scan(plan: &AskPlan) -> bool {
    plan.intents.iter().any(|intent| matches!(intent, Intent::ServerOverview | Intent::Findings))
}

pub fn plan_needs_knowledge_matches(plan: &AskPlan) -> bool {
    matches!(plan.depth, AnswerDepth::Deep)
        || plan.intents.iter().any(|intent| matches!(
            intent,
            Intent::GitLab | Intent::Kubernetes | Intent::Findings | Intent::Actions
        ))
}

pub fn intent_names(intents: &[Intent]) -> Vec<String> {
    intents.iter().map(|intent| format!("{intent:?}")).collect()
}

fn normalize_terms(question: &str) -> Vec<String> {
    question
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|term| !term.trim().is_empty())
        .map(normalize_term)
        .filter(|term| !term.is_empty())
        .collect()
}

fn normalize_term(term: &str) -> String {
    let lower = term.trim().to_ascii_lowercase();
    match lower.as_str() {
        "whats" | "what" | "wht" => "what".to_string(),
        "cpus" | "cores" => "cpu".to_string(),
        "processors" => "processor".to_string(),
        "mem" | "memeory" | "memry" | "ramm" => "memory".to_string(),
        "services" | "servise" | "servies" | "svc" => "service".to_string(),
        "processes" | "pids" => "process".to_string(),
        "apps" => "app".to_string(),
        "databases" | "postgresql" | "postgresh" | "postgress" => "postgres".to_string(),
        "ports" => "port".to_string(),
        "disks" | "filesystems" | "mounts" => "disk".to_string(),
        "pods" => "pod".to_string(),
        "nodes" => "node".to_string(),
        "issues" | "warnings" | "findings" | "problems" | "alerts" | "errors" => lower.trim_end_matches('s').to_string(),
        _ => lower,
    }
}

fn has_any(terms: &[String], needles: &[&str]) -> bool {
    terms.iter().any(|term| needles.iter().any(|needle| term == needle))
}

fn push_if(intents: &mut Vec<Intent>, intent: Intent, condition: bool) {
    if condition && !intents.contains(&intent) {
        intents.push(intent);
    }
}

fn has_relevant_warning_or_critical(intents: &[Intent], snapshot: &Snapshot) -> bool {
    intents.iter().any(|intent| match intent {
        Intent::Cpu => snapshot.compute.global_cpu_usage_percent >= 75.0
            || snapshot.findings.iter().any(|finding| finding_matches(finding, &["cpu", "processor", "load"])),
        Intent::Memory => percent(snapshot.memory.used_bytes, snapshot.memory.total_bytes) >= 75.0
            || snapshot.memory.used_swap_bytes > 0
            || snapshot.findings.iter().any(|finding| finding_matches(finding, &["memory", "ram", "swap"])),
        Intent::Storage => snapshot.storage.iter().any(|disk| disk.used_percent >= 80.0)
            || snapshot.findings.iter().any(|finding| finding_matches(finding, &["disk", "storage", "filesystem", "mount", "space"])),
        Intent::NetworkPorts => snapshot.findings.iter().any(|finding| finding_matches(finding, &["network", "port", "socket", "firewall"])),
        Intent::Processes => snapshot.top_processes.iter().any(|process| process.cpu_usage_percent >= 85.0)
            || snapshot.findings.iter().any(|finding| finding_matches(finding, &["process", "pid"])),
        Intent::Databases => snapshot.findings.iter().any(|finding| finding_matches(finding, &["database", "postgres", "mysql", "redis", "valkey", "mongo"])),
        Intent::Services => snapshot.findings.iter().any(|finding| finding_matches(finding, &["service", "systemd", "daemon"])),
        Intent::Kubernetes => snapshot.findings.iter().any(|finding| finding.category == "kubernetes"),
        Intent::GitLab => snapshot.findings.iter().any(|finding| finding.category == "gitlab"),
        Intent::Findings | Intent::ServerOverview => snapshot.findings.iter().any(is_warning_or_critical),
        Intent::Actions => true,
    })
}

fn finding_matches(finding: &Finding, keywords: &[&str]) -> bool {
    is_warning_or_critical(finding)
        && keywords.iter().any(|keyword| {
            finding.rule_id.contains(keyword)
                || finding.category.contains(keyword)
                || finding.title.to_ascii_lowercase().contains(keyword)
                || finding.detail.to_ascii_lowercase().contains(keyword)
        })
}

fn is_warning_or_critical(finding: &Finding) -> bool {
    finding.severity == "critical" || finding.severity == "warn"
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}
