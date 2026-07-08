// =============================================================================
// File: src/reasoning.rs
// Purpose:
//   Rule-based local reasoning layer that answers selected questions without calling Qwen.
//
// Where this fits in OSAI:
//   Supports fast, deterministic explanations from the current snapshot and knowledge base.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Use this for factual host reasoning; use Ask OSAI when Cognee memory and Qwen refinement are needed.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

use crate::{
    collector::{models::Finding, Snapshot},
    knowledge::{KnowledgeBase, KnowledgeMatch},
};

#[derive(Debug, Clone, Deserialize)]
pub struct ReasonRequest {
    pub question: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasonResponse {
    pub status: String,
    pub evidence: Vec<String>,
    pub likely_cause: String,
    pub safe_checks: Vec<String>,
    pub suggested_action: String,
    pub risk: String,
    pub matched_findings: Vec<Finding>,
    pub knowledge_matches: Vec<KnowledgeMatch>,
    pub note: String,
}

pub fn reason_about(question: &str, snapshot: &Snapshot, knowledge: &KnowledgeBase) -> ReasonResponse {
    let knowledge_matches = knowledge.search(question, 5);
    let matched_findings = match_findings(question, &snapshot.findings);
    let strongest = matched_findings
        .first()
        .or_else(|| snapshot.findings.first());

    let status = if snapshot.findings.iter().any(|f| f.severity == "critical") {
        "critical findings present"
    } else if snapshot.findings.iter().any(|f| f.severity == "warn") {
        "warnings present"
    } else if snapshot.findings.is_empty() {
        "no current findings"
    } else {
        "informational findings present"
    }
    .to_string();

    let evidence = strongest
        .map(|finding| finding.evidence.clone())
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| {
            vec![format!(
                "host={} os={} findings={}",
                snapshot.host.hostname,
                snapshot.os.long_version,
                snapshot.findings.len()
            )]
        });

    let likely_cause = strongest
        .map(|finding| finding.detail.clone())
        .unwrap_or_else(|| {
            "No direct rule matched this question. Use the loaded runbooks and current snapshot as context.".to_string()
        });

    let safe_checks = build_safe_checks(snapshot, strongest);
    let suggested_action = strongest
        .map(|finding| finding.recommendation.clone())
        .unwrap_or_else(|| "Run another scan, compare history, then inspect the most relevant service logs.".to_string());

    let risk = strongest
        .map(|finding| {
            if finding.requires_approval {
                "Changing state for this issue requires approval through the guarded action workflow.".to_string()
            } else {
                "Current suggestion is read-only. Repair actions still require explicit approval.".to_string()
            }
        })
        .unwrap_or_else(|| "No repair action selected. Keep investigation read-only first.".to_string());

    ReasonResponse {
        status,
        evidence,
        likely_cause,
        safe_checks,
        suggested_action,
        risk,
        matched_findings,
        knowledge_matches,
        note: "This is deterministic local reasoning over scanner output and Markdown knowledge. It is ready to connect to llama.cpp/Qwen later, but no external LLM is required for this phase.".to_string(),
    }
}

fn match_findings(question: &str, findings: &[Finding]) -> Vec<Finding> {
    let terms = question
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .map(|x| x.to_lowercase())
        .filter(|x| x.len() >= 3)
        .collect::<Vec<_>>();

    let mut scored = findings
        .iter()
        .cloned()
        .map(|finding| {
            let text = format!(
                "{} {} {} {} {}",
                finding.rule_id,
                finding.category,
                finding.title,
                finding.detail,
                finding.recommendation
            )
            .to_lowercase();
            let score = terms.iter().filter(|term| text.contains(term.as_str())).count();
            (score, finding)
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, finding)| finding).collect()
}

fn build_safe_checks(snapshot: &Snapshot, finding: Option<&Finding>) -> Vec<String> {
    let mut checks = Vec::new();

    if let Some(command) = finding.and_then(|f| f.command_suggestion.clone()) {
        checks.push(command);
    }

    if snapshot.kubernetes.detected {
        checks.extend(snapshot.kubernetes.safe_commands.clone());
    }

    if snapshot.gitlab.detected {
        checks.extend(snapshot.gitlab.safe_commands.clone());
    }

    if checks.is_empty() {
        checks.extend([
            "df -h".to_string(),
            "free -m".to_string(),
            "ss -tulpen".to_string(),
            "systemctl --failed".to_string(),
        ]);
    }

    checks.sort();
    checks.dedup();
    checks.truncate(8);
    checks
}
