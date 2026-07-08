// =============================================================================
// File: src/rules.rs
// Purpose:
//   Evaluates host facts into severity-tagged findings, evidence, and recommendations.
//
// Where this fits in OSAI:
//   Turns raw scanner data into operator-facing health/security signals.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Rules should include clear evidence and safe recommendations, especially when actions require approval.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use crate::collector::models::{
    ComputeInfo, DiskInfo, Finding, GitlabHint, KubernetesHint, ListeningPort, MemoryInfo,
    ProcessInfo,
};

pub struct RuleContext<'a> {
    pub memory: &'a MemoryInfo,
    pub compute: &'a ComputeInfo,
    pub storage: &'a [DiskInfo],
    pub ports: &'a [ListeningPort],
    pub kubernetes: &'a KubernetesHint,
    pub gitlab: &'a GitlabHint,
    pub top_processes: &'a [ProcessInfo],
}

pub fn evaluate_rules(ctx: RuleContext<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Each evaluator owns one operational domain. Keeping them separate makes
    // thresholds and recommendations easier to tune without touching scanning.
    evaluate_memory(ctx.memory, ctx.gitlab, ctx.top_processes, &mut findings);
    evaluate_cpu(ctx.compute, ctx.top_processes, &mut findings);
    evaluate_storage(ctx.storage, &mut findings);
    evaluate_ports(ctx.ports, &mut findings);
    evaluate_kubernetes(ctx.kubernetes, &mut findings);
    evaluate_gitlab(ctx.gitlab, ctx.memory, &mut findings);

    findings
}

fn evaluate_memory(
    memory: &MemoryInfo,
    gitlab: &GitlabHint,
    top_processes: &[ProcessInfo],
    findings: &mut Vec<Finding>,
) {
    if memory.total_bytes == 0 {
        return;
    }

    let used_percent = (memory.used_bytes as f64 / memory.total_bytes as f64) * 100.0;
    if used_percent >= 85.0 {
        let top = top_processes
            .first()
            .map(|p| format!(" Top process by memory: {} pid {}.", p.name, p.pid))
            .unwrap_or_default();

        findings.push(Finding {
            rule_id: "linux.memory.high".to_string(),
            severity: "warn".to_string(),
            category: "linux".to_string(),
            title: "High memory usage".to_string(),
            detail: format!("Memory usage is {:.1}%.{}", used_percent, top),
            evidence: vec![format!(
                "used={} total={} available={}",
                memory.used_bytes, memory.total_bytes, memory.available_bytes
            )],
            recommendation: "Check top memory processes, swap usage, and recent OOM messages before restarting anything.".to_string(),
            requires_approval: false,
            command_suggestion: Some("free -m".to_string()),
            plugin: Some("linux".to_string()),
        });
    }

    if gitlab.detected && used_percent >= 70.0 {
        // This encodes the known GitLab auto-start memory pattern so future
        // scans point back to the incident instead of treating it as generic RAM pressure.
        findings.push(Finding {
            rule_id: "gitlab.memory.autostart_regression".to_string(),
            severity: "warn".to_string(),
            category: "gitlab".to_string(),
            title: "GitLab may be consuming memory again".to_string(),
            detail: "GitLab signals are present while memory usage is elevated. Compare with the previous GitLab auto-start incident before suggesting unrelated fixes.".to_string(),
            evidence: gitlab.signals.clone(),
            recommendation: "Run read-only GitLab status checks first; stopping or disabling services requires approval.".to_string(),
            requires_approval: true,
            command_suggestion: Some("gitlab-ctl status".to_string()),
            plugin: Some("gitlab".to_string()),
        });
    }
}

fn evaluate_cpu(
    compute: &ComputeInfo,
    top_processes: &[ProcessInfo],
    findings: &mut Vec<Finding>,
) {
    if compute.global_cpu_usage_percent < 85.0 {
        return;
    }

    let top = top_processes
        .iter()
        .max_by(|a, b| {
            a.cpu_usage_percent
                .partial_cmp(&b.cpu_usage_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|p| format!(" Top CPU process: {} pid {} at {:.1}%.", p.name, p.pid, p.cpu_usage_percent))
        .unwrap_or_default();

    findings.push(Finding {
        rule_id: "linux.cpu.high".to_string(),
        severity: "warn".to_string(),
        category: "linux".to_string(),
        title: "High CPU usage".to_string(),
        detail: format!("Global CPU usage is {:.1}%.{}", compute.global_cpu_usage_percent, top),
        evidence: vec![format!(
            "logical_cpus={} physical_cores={:?}",
            compute.logical_cpus, compute.physical_cores
        )],
        recommendation: "Identify the busy process, check recent logs, and only restart after confirming impact.".to_string(),
        requires_approval: false,
        command_suggestion: Some("ps aux --sort=-%cpu".to_string()),
        plugin: Some("linux".to_string()),
    });
}

fn evaluate_storage(storage: &[DiskInfo], findings: &mut Vec<Finding>) {
    for disk in storage {
        if disk.used_percent >= 90.0 {
            findings.push(Finding {
                rule_id: "linux.disk.critical".to_string(),
                severity: "critical".to_string(),
                category: "linux".to_string(),
                title: "Critical disk usage".to_string(),
                detail: format!("{} is {:.1}% used.", disk.mount_point, disk.used_percent),
                evidence: vec![format!(
                    "mount={} total={} available={}",
                    disk.mount_point, disk.total_bytes, disk.available_bytes
                )],
                recommendation: "Find the largest directories and logs. Do not delete automatically.".to_string(),
                requires_approval: true,
                command_suggestion: Some(format!("du -xh {} --max-depth=1", disk.mount_point)),
                plugin: Some("linux".to_string()),
            });
        } else if disk.used_percent >= 80.0 {
            findings.push(Finding {
                rule_id: "linux.disk.warn".to_string(),
                severity: "warn".to_string(),
                category: "linux".to_string(),
                title: "High disk usage".to_string(),
                detail: format!("{} is {:.1}% used.", disk.mount_point, disk.used_percent),
                evidence: vec![format!(
                    "mount={} total={} available={}",
                    disk.mount_point, disk.total_bytes, disk.available_bytes
                )],
                recommendation: "Check log growth, container images, database files, and backups.".to_string(),
                requires_approval: false,
                command_suggestion: Some("df -h".to_string()),
                plugin: Some("linux".to_string()),
            });
        }
    }
}

fn evaluate_ports(ports: &[ListeningPort], findings: &mut Vec<Finding>) {
    // These ports are not automatically bad, but they deserve visibility because
    // exposing databases, Kubernetes, or Docker APIs publicly is high risk.
    let sensitive_ports = [2375, 2376, 5432, 3306, 6379, 9200, 10250, 6443];

    for port in ports {
        if !sensitive_ports.contains(&port.port) {
            continue;
        }

        let category = match port.port {
            6443 | 10250 => "kubernetes",
            5432 | 3306 | 6379 | 9200 => "database",
            2375 | 2376 => "container-runtime",
            _ => "network",
        };

        findings.push(Finding {
            rule_id: format!("network.sensitive_port.{}", port.port),
            severity: "info".to_string(),
            category: category.to_string(),
            title: "Sensitive service port detected".to_string(),
            detail: format!(
                "{} port {} is visible in /proc/net. Confirm firewall and bind address.",
                port.protocol, port.port
            ),
            evidence: vec![format!(
                "protocol={} address_raw={} state={}",
                port.protocol, port.local_address_raw, port.state
            )],
            recommendation: "Confirm the service is bound to the expected interface and protected by host firewall/security groups.".to_string(),
            requires_approval: false,
            command_suggestion: Some("ss -tulpen".to_string()),
            plugin: Some("network".to_string()),
        });
    }
}

fn evaluate_kubernetes(kubernetes: &KubernetesHint, findings: &mut Vec<Finding>) {
    if !kubernetes.detected {
        return;
    }

    findings.push(Finding {
        rule_id: "kubernetes.detected".to_string(),
        severity: "info".to_string(),
        category: "kubernetes".to_string(),
        title: "Kubernetes signals detected".to_string(),
        detail: kubernetes.summary.clone(),
        evidence: kubernetes.signals.clone(),
        recommendation: "Use read-only kubectl checks first: nodes, pods, events, and node pressure.".to_string(),
        requires_approval: false,
        command_suggestion: Some("kubectl get pods -A -o wide".to_string()),
        plugin: Some("kubernetes".to_string()),
    });
}

fn evaluate_gitlab(gitlab: &GitlabHint, memory: &MemoryInfo, findings: &mut Vec<Finding>) {
    if !gitlab.detected {
        return;
    }

    findings.push(Finding {
        rule_id: "gitlab.detected".to_string(),
        severity: "info".to_string(),
        category: "gitlab".to_string(),
        title: "GitLab signals detected".to_string(),
        detail: gitlab.summary.clone(),
        evidence: gitlab.signals.clone(),
        recommendation: "Use GitLab incident memory before changing services.".to_string(),
        requires_approval: false,
        command_suggestion: Some("gitlab-ctl status".to_string()),
        plugin: Some("gitlab".to_string()),
    });

    if memory.used_swap_bytes > 0 {
        findings.push(Finding {
            rule_id: "gitlab.swap.used".to_string(),
            severity: "warn".to_string(),
            category: "gitlab".to_string(),
            title: "GitLab host is using swap".to_string(),
            detail: "Swap is in use on a host where GitLab signals were detected.".to_string(),
            evidence: vec![format!(
                "used_swap={} total_swap={}",
                memory.used_swap_bytes, memory.total_swap_bytes
            )],
            recommendation: "Check whether GitLab components started unexpectedly and inspect top memory consumers.".to_string(),
            requires_approval: false,
            command_suggestion: Some("free -m".to_string()),
            plugin: Some("gitlab".to_string()),
        });
    }
}
