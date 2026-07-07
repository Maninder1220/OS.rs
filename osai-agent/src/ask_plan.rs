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

use crate::collector::Snapshot;

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

    push_if(&mut intents, Intent::Cpu, has_any(&terms, &["cpu", "core", "cores", "processor", "processors", "load"]));
    push_if(&mut intents, Intent::Memory, has_any(&terms, &["ram", "memory", "swap"]));
    push_if(&mut intents, Intent::Storage, has_any(&terms, &["disk", "disks", "storage", "filesystem", "mount", "space"]));
    push_if(&mut intents, Intent::Services, has_any(&terms, &["service", "services", "daemon", "systemd"]));
    push_if(&mut intents, Intent::Processes, has_any(&terms, &["process", "processes", "pid", "top", "app", "apps"]));
    push_if(&mut intents, Intent::Databases, has_any(&terms, &["database", "databases", "db", "postgres", "postgresql", "mysql", "redis", "valkey", "mongo"]));
    push_if(&mut intents, Intent::NetworkPorts, has_any(&terms, &["network", "port", "ports", "listening", "socket", "firewall"]));
    push_if(&mut intents, Intent::Findings, has_any(&terms, &["issue", "issues", "warning", "warnings", "critical", "finding", "findings", "problem", "problems", "alert"]));
    push_if(&mut intents, Intent::Kubernetes, has_any(&terms, &["kubernetes", "k8s", "kubectl", "pod", "pods", "node", "nodes"]));
    push_if(&mut intents, Intent::GitLab, has_any(&terms, &["gitlab", "gitaly", "workhorse", "git"]));
    push_if(&mut intents, Intent::Actions, has_any(&terms, &["action", "actions", "fix", "repair", "approve", "run"]));

    let focused = !intents.is_empty();
    let asks_overview = has_any(&terms, &["update", "overview", "status", "health", "server", "host", "machine", "system"]);
    if !focused && asks_overview {
        intents.push(Intent::ServerOverview);
    }
    if intents.is_empty() {
        intents.push(Intent::ServerOverview);
    }

    let has_risk = snapshot.findings.iter().any(|finding| finding.severity == "critical" || finding.severity == "warn");
    let historical_words = has_any(&terms, &["before", "previous", "past", "again", "repeat", "repeated", "incident", "history", "resolved", "failed", "failing"]);
    let use_cognee = historical_words
        || has_risk
        || intents.iter().any(|intent| matches!(
            intent,
            Intent::GitLab | Intent::Kubernetes | Intent::Services | Intent::Findings | Intent::Actions
        ));

    let depth = if has_any(&terms, &["deep", "detail", "details", "explain", "why"]) {
        AnswerDepth::Deep
    } else if focused {
        AnswerDepth::Short
    } else {
        AnswerDepth::Normal
    };

    let response_style = if has_risk || intents.contains(&Intent::Findings) {
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
            "Focused intent detected; FactPack should avoid unrelated host data.".to_string()
        } else {
            "No focused intent detected; use compact server overview facts.".to_string()
        },
    }
}

fn normalize_terms(question: &str) -> Vec<String> {
    question
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|term| !term.trim().is_empty())
        .map(|term| term.trim().to_ascii_lowercase())
        .collect()
}

fn has_any(terms: &[String], needles: &[&str]) -> bool {
    terms.iter().any(|term| needles.iter().any(|needle| term == needle))
}

fn push_if(intents: &mut Vec<Intent>, intent: Intent, condition: bool) {
    if condition && !intents.contains(&intent) {
        intents.push(intent);
    }
}
