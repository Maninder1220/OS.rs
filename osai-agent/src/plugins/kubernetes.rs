// =============================================================================
// File: src/plugins/kubernetes.rs
// Purpose:
//   Detects Kubernetes-related process signals and suggests safe cluster inspection commands.
//
// Where this fits in OSAI:
//   Adds Kubernetes awareness to scanner output and rules without requiring kubectl execution.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Detection is signal-based; availability and permissions must still be verified by the operator.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::{collections::BTreeSet, path::Path};

use crate::collector::models::KubernetesHint;

pub fn collect_kubernetes_hints(processes: &BTreeSet<String>) -> KubernetesHint {
    let mut signals = Vec::new();
    let mut safe_commands = Vec::new();

    for name in ["kubelet", "kube-apiserver", "containerd", "etcd", "kube-proxy"] {
        if contains_process(processes, name) {
            signals.push(format!("process detected: {name}"));
        }
    }

    for path in [
        "/etc/kubernetes",
        "/etc/kubernetes/manifests",
        "/var/lib/kubelet",
        "/var/lib/etcd",
        "/root/.kube/config",
    ] {
        if Path::new(path).exists() {
            signals.push(format!("path exists: {path}"));
        }
    }

    for kubectl in ["/usr/bin/kubectl", "/usr/local/bin/kubectl", "/snap/bin/kubectl"] {
        if Path::new(kubectl).exists() {
            signals.push(format!("kubectl binary found: {kubectl}"));
        }
    }

    if !signals.is_empty() {
        safe_commands.extend([
            "kubectl get nodes -o wide".to_string(),
            "kubectl get pods -A -o wide".to_string(),
            "kubectl get events -A --sort-by=.lastTimestamp".to_string(),
        ]);
    }

    KubernetesHint {
        detected: !signals.is_empty(),
        available: !signals.is_empty(),
        summary: if signals.is_empty() {
            "No Kubernetes process, path, or kubectl signal was detected.".to_string()
        } else {
            "Kubernetes plugin detected cluster-related signals. Only read-only cluster checks are suggested by default.".to_string()
        },
        signals,
        safe_commands,
    }
}

fn contains_process(processes: &BTreeSet<String>, needle: &str) -> bool {
    processes.iter().any(|name| name.contains(needle))
}
