// =============================================================================
// File: src/intent.rs
// Purpose:
//   Classifies operator questions into safe intents and maps them to evidence-backed answers/actions.
//
// Where this fits in OSAI:
//   Used by reasoning and UI flows to keep assistant behavior grounded in scanned facts.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Intent logic should be explainable and conservative because it influences suggested operations.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use serde::Serialize;

use crate::collector::Snapshot;

#[derive(Debug, Clone, Serialize)]
pub struct QueryInsight {
    pub id: String,
    pub label: String,
    pub status: String,
    pub severity: String,
    pub summary: String,
    pub metrics: Vec<InsightMetric>,
    pub recommendation: String,
    pub manual_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightMetric {
    pub label: String,
    pub value: String,
    pub unit: String,
    pub percent: Option<f64>,
}

pub fn analyze_question(question: &str, snapshot: &Snapshot) -> Vec<QueryInsight> {
    let terms = normalize_terms(question);
    let full_update = wants_full_update(&terms);
    let mut insights = Vec::new();

    // The full-update path intentionally returns broad server status first. It
    // gives users a useful answer even when they ask in casual language.
    if full_update {
        insights.push(server_overview_insight(snapshot));
        let findings = findings_insight(snapshot);
        if findings.severity == "warn" || findings.severity == "critical" {
            insights.push(findings);
        }
        return insights;
    }

    // From here down, words in the user's question select specific scan views.
    // This keeps the response stable and explainable before optional Qwen refinement.
    if has_any(&terms, &["server", "host", "machine", "os", "system", "uptime"]) {
        insights.push(server_overview_insight(snapshot));
    }

    if has_any(&terms, &["core", "cores", "cpu", "processor", "processors", "load"]) {
        insights.push(cpu_insight(snapshot));
    }

    if has_any(&terms, &["memory", "ram", "swap"]) {
        insights.push(memory_insight(snapshot));
    }

    if has_any(&terms, &["disk", "disks", "storage", "filesystem", "mount", "space"]) {
        insights.push(storage_insight(snapshot));
    }

    if has_any(&terms, &["network", "port", "ports", "listening", "socket", "firewall"]) {
        insights.push(network_ports_insight(snapshot));
    }

    if has_any(&terms, &["process", "processes", "top", "pid", "service", "services", "app", "apps"]) {
        insights.push(process_insight(snapshot));
        insights.push(services_insight(snapshot));
    }

    if has_any(&terms, &["database", "databases", "db", "postgres", "postgresql", "mysql", "redis", "valkey", "mongo"]) {
        insights.push(services_insight(snapshot));
    }

    if has_any(&terms, &["finding", "findings", "warning", "warnings", "critical", "issue", "issues", "problem", "problems", "alert"]) {
        insights.push(findings_insight(snapshot));
    }

    if has_any(&terms, &["kubernetes", "k8s", "kubectl", "pod", "pods", "node", "nodes"]) {
        insights.push(kubernetes_insight(snapshot));
    }

    if has_any(&terms, &["gitlab", "gitaly", "workhorse", "git"]) {
        insights.push(gitlab_insight(snapshot));
    }

    dedupe_insights(insights)
}

pub fn build_deterministic_answer(question: &str, insights: &[QueryInsight], snapshot: &Snapshot) -> String {
    if insights.is_empty() {
        return [
            "I checked the current scan, but I could not map your question to a known local signal yet.".to_string(),
            String::new(),
            format!("Question: {question}"),
            format!(
                "Current scan: host={} os={} findings={}",
                snapshot.host.hostname,
                snapshot.os.long_version,
                snapshot.findings.len()
            ),
            String::new(),
            "Try asking: whats the update, CPU status, memory status, disk usage, open ports, top processes, services, databases, Kubernetes, GitLab, or findings. The UI can add these views when you need them.".to_string(),
        ]
        .join("\n");
    }

    let mut lines = vec![
        "Rust matched your question with the current machine scan.".to_string(),
        String::new(),
        format!("Question: {question}"),
        String::new(),
    ];

    for insight in insights {
        lines.push(format!("## {}", insight.label));
        lines.push(format!("Status: {} ({})", insight.status, insight.severity));
        lines.push(insight.summary.clone());
        for metric in &insight.metrics {
            let percent = metric
                .percent
                .map(|value| format!(" / {:.1}%", value))
                .unwrap_or_default();
            lines.push(format!("- {}: {}{}{}", metric.label, metric.value, metric.unit, percent));
        }
        if !insight.manual_checks.is_empty() {
            lines.push("Manual checks:".to_string());
            for command in &insight.manual_checks {
                lines.push(format!("- {command}"));
            }
        }
        lines.push(format!("Recommendation: {}", insight.recommendation));
        lines.push(String::new());
    }

    lines.push("This answer is deterministic Rust reasoning over scanner data. Turn AI on only when you want Qwen to refine the same evidence.".to_string());
    lines.join("\n")
}

fn server_overview_insight(snapshot: &Snapshot) -> QueryInsight {
    let critical = snapshot.findings.iter().filter(|f| f.severity == "critical").count();
    let warnings = snapshot.findings.iter().filter(|f| f.severity == "warn").count();
    let severity = if critical > 0 {
        "critical"
    } else if warnings > 0 {
        "warn"
    } else {
        "ok"
    };
    let status = if critical > 0 {
        "critical attention"
    } else if warnings > 0 {
        "needs attention"
    } else {
        "stable"
    };

    QueryInsight {
        id: "server_overview".to_string(),
        label: "Full Server Overview".to_string(),
        status: status.to_string(),
        severity: severity.to_string(),
        summary: format!(
            "{} is running {} with uptime {}. Rust found {} findings, {} listening ports, {} mounted filesystems, and {} tracked processes.",
            snapshot.host.hostname,
            snapshot.os.long_version,
            human_duration(snapshot.host.uptime_seconds),
            snapshot.findings.len(),
            snapshot.listening_ports.len(),
            snapshot.storage.len(),
            snapshot.top_processes.len()
        ),
        metrics: vec![
            InsightMetric {
                label: "Hostname".to_string(),
                value: snapshot.host.hostname.clone(),
                unit: String::new(),
                percent: None,
            },
            InsightMetric {
                label: "OS".to_string(),
                value: snapshot.os.long_version.clone(),
                unit: String::new(),
                percent: None,
            },
            InsightMetric {
                label: "Uptime".to_string(),
                value: human_duration(snapshot.host.uptime_seconds),
                unit: String::new(),
                percent: None,
            },
            InsightMetric {
                label: "Findings".to_string(),
                value: snapshot.findings.len().to_string(),
                unit: String::new(),
                percent: None,
            },
        ],
        recommendation: if severity == "ok" {
            "Server overview looks stable. Use labels below for in-depth CPU, memory, disk, ports, and services.".to_string()
        } else {
            "Start with critical/warning findings, then inspect the matching CPU, memory, disk, ports, or service labels.".to_string()
        },
        manual_checks: vec![
            "hostnamectl".to_string(),
            "uptime".to_string(),
            "df -h".to_string(),
            "free -m".to_string(),
            "ss -tulpen".to_string(),
            "systemctl --failed".to_string(),
        ],
    }
}

fn cpu_insight(snapshot: &Snapshot) -> QueryInsight {
    let usage = snapshot.compute.global_cpu_usage_percent as f64;
    let high_cores = snapshot
        .compute
        .cpus
        .iter()
        .filter(|cpu| cpu.usage_percent >= 85.0)
        .count();

    let (status, severity, recommendation) = classify_percent(
        usage,
        "CPU/core usage is low. No immediate attention needed.",
        "CPU/core usage is normal. Keep watching if workload is expected to grow.",
        "CPU/core usage needs attention. Check top CPU processes before restarting anything.",
        "CPU/core usage is high. Investigate busy processes and recent workload changes now.",
    );

    QueryInsight {
        id: "cpu_core".to_string(),
        label: "CPU / Core Utilization".to_string(),
        status,
        severity,
        summary: format!(
            "Rust mapped CPU/core words to processor utilization. Current scan reports {:.1}% global CPU usage across {} logical CPUs.",
            usage,
            snapshot.compute.logical_cpus
        ),
        metrics: vec![
            metric_percent("Global CPU usage", usage),
            metric_count("Logical CPUs", snapshot.compute.logical_cpus),
            InsightMetric {
                label: "Physical cores".to_string(),
                value: snapshot
                    .compute
                    .physical_cores
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                unit: String::new(),
                percent: None,
            },
            metric_count("Cores above 85%", high_cores),
        ],
        recommendation,
        manual_checks: vec![
            "uptime".to_string(),
            "ps aux --sort=-%cpu".to_string(),
            "top -o %CPU".to_string(),
        ],
    }
}

fn memory_insight(snapshot: &Snapshot) -> QueryInsight {
    let used_percent = percent(snapshot.memory.used_bytes, snapshot.memory.total_bytes);
    let swap_percent = percent(snapshot.memory.used_swap_bytes, snapshot.memory.total_swap_bytes);

    let (status, severity, recommendation) = classify_percent(
        used_percent,
        "Memory usage is low. No immediate attention needed.",
        "Memory usage is normal. Keep watching if the workload is growing.",
        "Memory usage needs attention. Check top memory processes and swap pressure.",
        "Memory usage is high. Investigate top memory processes and possible service pressure now.",
    );

    QueryInsight {
        id: "memory".to_string(),
        label: "Memory / RAM Utilization".to_string(),
        status,
        severity,
        summary: format!(
            "Rust mapped memory/RAM words to RAM and swap usage. Current scan reports {:.1}% memory usage.",
            used_percent
        ),
        metrics: vec![
            metric_bytes_percent("Memory used", snapshot.memory.used_bytes, used_percent),
            metric_bytes("Memory total", snapshot.memory.total_bytes),
            metric_bytes("Memory available", snapshot.memory.available_bytes),
            metric_bytes_percent("Swap used", snapshot.memory.used_swap_bytes, swap_percent),
        ],
        recommendation,
        manual_checks: vec![
            "free -m".to_string(),
            "cat /proc/meminfo".to_string(),
            "ps aux --sort=-%mem".to_string(),
        ],
    }
}

fn storage_insight(snapshot: &Snapshot) -> QueryInsight {
    let worst = snapshot
        .storage
        .iter()
        .max_by(|a, b| a.used_percent.partial_cmp(&b.used_percent).unwrap_or(std::cmp::Ordering::Equal));
    let worst_percent = worst.map(|disk| disk.used_percent).unwrap_or(0.0);
    let (status, severity, recommendation) = classify_percent(
        worst_percent,
        "Disk usage is low. No immediate storage pressure found.",
        "Disk usage is normal. Keep monitoring log and container growth.",
        "Storage needs attention. Check largest directories and fast-growing logs.",
        "Storage usage is high. Investigate before writes, databases, or containers are impacted.",
    );

    let mut metrics = vec![metric_count("Mounted filesystems", snapshot.storage.len())];
    if let Some(disk) = worst {
        metrics.extend([
            InsightMetric {
                label: "Most used mount".to_string(),
                value: disk.mount_point.clone(),
                unit: String::new(),
                percent: Some(disk.used_percent),
            },
            metric_percent("Most used percent", disk.used_percent),
            metric_bytes("Most used total", disk.total_bytes),
            metric_bytes("Most used available", disk.available_bytes),
        ]);
    }

    QueryInsight {
        id: "storage".to_string(),
        label: "Storage / Disk Usage".to_string(),
        status,
        severity,
        summary: match worst {
            Some(disk) => format!(
                "Rust checked {} mounted filesystems. The most used mount is {} at {:.1}% used.",
                snapshot.storage.len(),
                disk.mount_point,
                disk.used_percent
            ),
            None => "Rust did not receive disk information in the current scan.".to_string(),
        },
        metrics,
        recommendation,
        manual_checks: vec![
            "df -h".to_string(),
            "df -ih".to_string(),
            "du -xh / --max-depth=1".to_string(),
        ],
    }
}

fn network_ports_insight(snapshot: &Snapshot) -> QueryInsight {
    let sensitive_count = snapshot
        .listening_ports
        .iter()
        .filter(|port| is_sensitive_port(port.port))
        .count();
    let severity = if sensitive_count > 0 { "warn" } else { "ok" };
    let status = if sensitive_count > 0 {
        "needs review"
    } else {
        "normal"
    };

    QueryInsight {
        id: "network_ports".to_string(),
        label: "Network / Listening Ports".to_string(),
        status: status.to_string(),
        severity: severity.to_string(),
        summary: format!(
            "Rust found {} listening sockets and {} network interfaces. {} sensitive ports should be checked for bind address and firewall exposure.",
            snapshot.listening_ports.len(),
            snapshot.network.len(),
            sensitive_count
        ),
        metrics: vec![
            metric_count("Network interfaces", snapshot.network.len()),
            metric_count("Listening ports", snapshot.listening_ports.len()),
            metric_count("Sensitive ports", sensitive_count),
        ],
        recommendation: if sensitive_count > 0 {
            "Review sensitive ports with ss -tulpen and confirm firewall/security group exposure.".to_string()
        } else {
            "No sensitive listening ports were highlighted by the current rule set.".to_string()
        },
        manual_checks: vec![
            "ss -tulpen".to_string(),
            "ip addr".to_string(),
            "ip route".to_string(),
            "firewall-cmd --list-all".to_string(),
        ],
    }
}

fn process_insight(snapshot: &Snapshot) -> QueryInsight {
    let top_memory = snapshot.top_processes.first();
    let top_cpu = snapshot.top_processes.iter().max_by(|a, b| {
        a.cpu_usage_percent
            .partial_cmp(&b.cpu_usage_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_cpu_percent = top_cpu.map(|process| process.cpu_usage_percent as f64).unwrap_or(0.0);
    let severity = if top_cpu_percent >= 85.0 { "warn" } else { "ok" };
    let status = if top_cpu_percent >= 85.0 { "busy process" } else { "normal" };

    QueryInsight {
        id: "processes".to_string(),
        label: "Top Processes".to_string(),
        status: status.to_string(),
        severity: severity.to_string(),
        summary: format!(
            "Rust is tracking the top {} processes sorted by memory and CPU. Top memory process: {}. Top CPU process: {}.",
            snapshot.top_processes.len(),
            top_memory
                .map(|p| format!("{} pid {}", p.name, p.pid))
                .unwrap_or_else(|| "unknown".to_string()),
            top_cpu
                .map(|p| format!("{} pid {} at {:.1}%", p.name, p.pid, p.cpu_usage_percent))
                .unwrap_or_else(|| "unknown".to_string())
        ),
        metrics: vec![
            metric_count("Tracked top processes", snapshot.top_processes.len()),
            InsightMetric {
                label: "Top memory process".to_string(),
                value: top_memory
                    .map(|p| format!("{} ({})", p.name, human_bytes(p.memory_bytes)))
                    .unwrap_or_else(|| "unknown".to_string()),
                unit: String::new(),
                percent: None,
            },
            InsightMetric {
                label: "Top CPU process".to_string(),
                value: top_cpu
                    .map(|p| format!("{} ({:.1}%)", p.name, p.cpu_usage_percent))
                    .unwrap_or_else(|| "unknown".to_string()),
                unit: String::new(),
                percent: Some(top_cpu_percent),
            },
        ],
        recommendation: if severity == "warn" {
            "Inspect the top CPU process and related service logs before taking repair action.".to_string()
        } else {
            "Process pressure looks normal from the current top-process sample.".to_string()
        },
        manual_checks: vec![
            "ps aux --sort=-%mem".to_string(),
            "ps aux --sort=-%cpu".to_string(),
            "systemctl --failed".to_string(),
        ],
    }
}

fn services_insight(snapshot: &Snapshot) -> QueryInsight {
    let services = names_from_services(snapshot);
    let apps = names_from_apps(&snapshot.app_hints);
    let databases = names_from_apps(&snapshot.database_hints);
    let detected_total = snapshot.service_hints.len() + snapshot.app_hints.len() + snapshot.database_hints.len();

    QueryInsight {
        id: "services_apps_databases".to_string(),
        label: "Services / Apps / Databases".to_string(),
        status: if detected_total > 0 { "detected" } else { "quiet" }.to_string(),
        severity: "ok".to_string(),
        summary: format!(
            "Rust detected services [{}], apps [{}], and databases [{}] from process and port hints.",
            services,
            apps,
            databases
        ),
        metrics: vec![
            metric_count("Service hints", snapshot.service_hints.len()),
            metric_count("App hints", snapshot.app_hints.len()),
            metric_count("Database hints", snapshot.database_hints.len()),
        ],
        recommendation: if detected_total > 0 {
            "Use labels or ask by service name for deeper checks. Keep repair actions in the guarded action workflow.".to_string()
        } else {
            "No known app/database hints were found in the current scan.".to_string()
        },
        manual_checks: vec![
            "systemctl --failed".to_string(),
            "systemctl list-units --type=service --state=running".to_string(),
            "ss -tulpen".to_string(),
            "journalctl -p warning --since \"1 hour ago\"".to_string(),
        ],
    }
}

fn findings_insight(snapshot: &Snapshot) -> QueryInsight {
    let critical = snapshot.findings.iter().filter(|f| f.severity == "critical").count();
    let warnings = snapshot.findings.iter().filter(|f| f.severity == "warn").count();
    let severity = if critical > 0 {
        "critical"
    } else if warnings > 0 {
        "warn"
    } else {
        "ok"
    };
    let status = if critical > 0 {
        "critical findings"
    } else if warnings > 0 {
        "warnings present"
    } else {
        "no current findings"
    };

    QueryInsight {
        id: "findings".to_string(),
        label: "Rule Findings".to_string(),
        status: status.to_string(),
        severity: severity.to_string(),
        summary: format!(
            "Rust rules produced {} findings: {} critical, {} warning.",
            snapshot.findings.len(),
            critical,
            warnings
        ),
        metrics: vec![
            metric_count("Total findings", snapshot.findings.len()),
            metric_count("Critical", critical),
            metric_count("Warnings", warnings),
        ],
        recommendation: if severity == "ok" {
            "No current rule findings. Continue with normal monitoring.".to_string()
        } else {
            "Open the Findings panel first, then ask about the matching category for deeper detail.".to_string()
        },
        manual_checks: vec![
            "systemctl --failed".to_string(),
            "journalctl -p warning --since \"1 hour ago\"".to_string(),
            "df -h".to_string(),
            "free -m".to_string(),
        ],
    }
}

fn kubernetes_insight(snapshot: &Snapshot) -> QueryInsight {
    QueryInsight {
        id: "kubernetes".to_string(),
        label: "Kubernetes Signals".to_string(),
        status: if snapshot.kubernetes.detected { "detected" } else { "not detected" }.to_string(),
        severity: if snapshot.kubernetes.detected { "ok" } else { "info" }.to_string(),
        summary: snapshot.kubernetes.summary.clone(),
        metrics: vec![
            metric_count("Signals", snapshot.kubernetes.signals.len()),
            metric_count("Safe commands", snapshot.kubernetes.safe_commands.len()),
        ],
        recommendation: if snapshot.kubernetes.detected {
            "Use read-only kubectl checks first: nodes, pods, events, and node pressure.".to_string()
        } else {
            "No Kubernetes signal was found in the current process scan.".to_string()
        },
        manual_checks: vec![
            "kubectl get nodes -o wide".to_string(),
            "kubectl get pods -A -o wide".to_string(),
            "kubectl get events -A --sort-by=.lastTimestamp".to_string(),
            "kubectl top nodes".to_string(),
        ],
    }
}

fn gitlab_insight(snapshot: &Snapshot) -> QueryInsight {
    QueryInsight {
        id: "gitlab".to_string(),
        label: "GitLab Signals".to_string(),
        status: if snapshot.gitlab.detected { "detected" } else { "not detected" }.to_string(),
        severity: if snapshot.gitlab.detected { "ok" } else { "info" }.to_string(),
        summary: snapshot.gitlab.summary.clone(),
        metrics: vec![
            metric_count("Signals", snapshot.gitlab.signals.len()),
            metric_count("Safe commands", snapshot.gitlab.safe_commands.len()),
        ],
        recommendation: if snapshot.gitlab.detected {
            "Compare with GitLab incident memory before stopping or disabling services.".to_string()
        } else {
            "No GitLab signal was found in the current process scan.".to_string()
        },
        manual_checks: vec![
            "gitlab-ctl status".to_string(),
            "gitlab-ctl tail".to_string(),
            "systemctl status gitlab-runsvdir".to_string(),
            "free -m".to_string(),
        ],
    }
}

fn wants_full_update(terms: &[String]) -> bool {
    terms.is_empty()
        || has_all(terms, &["whats", "update"])
        || has_all(terms, &["what", "update"])
        || has_all(terms, &["server", "update"])
        || has_any(terms, &["overview", "summary", "health", "everything", "all"])
}

fn dedupe_insights(insights: Vec<QueryInsight>) -> Vec<QueryInsight> {
    let mut seen = Vec::new();
    let mut deduped = Vec::new();
    for insight in insights {
        if seen.iter().any(|id| id == &insight.id) {
            continue;
        }
        seen.push(insight.id.clone());
        deduped.push(insight);
    }
    deduped
}

fn names_from_services(snapshot: &Snapshot) -> String {
    if snapshot.service_hints.is_empty() {
        "none".to_string()
    } else {
        snapshot
            .service_hints
            .iter()
            .map(|item| item.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn names_from_apps(items: &[crate::collector::models::AppHint]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items
            .iter()
            .map(|item| item.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn classify_percent(
    value: f64,
    low_recommendation: &str,
    normal_recommendation: &str,
    attention_recommendation: &str,
    high_recommendation: &str,
) -> (String, String, String) {
    if value >= 85.0 {
        (
            "high use".to_string(),
            "critical".to_string(),
            high_recommendation.to_string(),
        )
    } else if value >= 70.0 {
        (
            "needs attention".to_string(),
            "warn".to_string(),
            attention_recommendation.to_string(),
        )
    } else if value >= 35.0 {
        (
            "normal use".to_string(),
            "ok".to_string(),
            normal_recommendation.to_string(),
        )
    } else {
        (
            "low use".to_string(),
            "ok".to_string(),
            low_recommendation.to_string(),
        )
    }
}

fn normalize_terms(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|term| term.trim().to_lowercase())
        .map(|term| if term == "what" || term == "what's" { "whats".to_string() } else { term })
        .filter(|term| !term.is_empty())
        .collect()
}

fn has_any(terms: &[String], keywords: &[&str]) -> bool {
    terms
        .iter()
        .any(|term| keywords.iter().any(|keyword| term == keyword))
}

fn has_all(terms: &[String], keywords: &[&str]) -> bool {
    keywords
        .iter()
        .all(|keyword| terms.iter().any(|term| term == keyword))
}

fn is_sensitive_port(port: u16) -> bool {
    matches!(port, 2375 | 2376 | 3306 | 5432 | 6379 | 6443 | 9200 | 10250)
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}

fn metric_percent(label: &str, value: f64) -> InsightMetric {
    InsightMetric {
        label: label.to_string(),
        value: format!("{value:.1}"),
        unit: "%".to_string(),
        percent: Some(value),
    }
}

fn metric_count(label: &str, value: usize) -> InsightMetric {
    InsightMetric {
        label: label.to_string(),
        value: value.to_string(),
        unit: String::new(),
        percent: None,
    }
}

fn metric_bytes(label: &str, value: u64) -> InsightMetric {
    InsightMetric {
        label: label.to_string(),
        value: human_bytes(value),
        unit: String::new(),
        percent: None,
    }
}

fn metric_bytes_percent(label: &str, value: u64, percent: f64) -> InsightMetric {
    InsightMetric {
        label: label.to_string(),
        value: human_bytes(value),
        unit: String::new(),
        percent: Some(percent),
    }
}

fn human_bytes(value: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = value as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < units.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if size >= 10.0 {
        format!("{size:.1} {}", units[unit])
    } else {
        format!("{size:.2} {}", units[unit])
    }
}

fn human_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}
