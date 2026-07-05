// =============================================================================
// File: src/collector/scanner.rs
// Purpose:
//   Collects host facts: OS, CPU, memory, disks, network, ports, processes, plugins, and rule findings.
//
// Where this fits in OSAI:
//   Produces the Snapshot consumed by the API, storage worker, rules, and Ask OSAI context.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Scanner code should prefer safe system APIs and typed outputs over shell commands.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::collections::BTreeSet;

use chrono::Utc;
use sysinfo::{Disks, Networks, System};

use crate::{
    plugins::{collect_gitlab_hints, collect_kubernetes_hints},
    rules::{evaluate_rules, RuleContext},
};

use super::{
    models::{
        AppHint, ComputeInfo, CpuInfo, DiskInfo, HostInfo, MemoryInfo, NetworkInfo, OsInfo,
        ProcessInfo, ServiceHint, Snapshot,
    },
    ports::collect_listening_ports,
};

pub async fn collect_snapshot() -> Snapshot {
    let mut system = System::new_all();

    // CPU usage is more accurate after a small delay between refreshes.
    system.refresh_all();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    system.refresh_all();

    let disks = Disks::new_with_refreshed_list();
    let networks = Networks::new_with_refreshed_list();

    // Build normalized signal groups first. Later layers should consume these
    // typed structs instead of rereading /proc or shelling out again.
    let top_processes = collect_top_processes(&system);
    let process_name_set = process_name_set(&top_processes);

    let storage = collect_disks(&disks);
    let network = collect_networks(&networks);
    let listening_ports = collect_listening_ports();

    let service_hints = collect_service_hints(&process_name_set);
    let app_hints = collect_app_hints(&process_name_set);
    let database_hints = collect_database_hints(&process_name_set, &listening_ports);
    let kubernetes = collect_kubernetes_hints(&process_name_set);
    let gitlab = collect_gitlab_hints(&process_name_set);

    let memory = MemoryInfo {
        total_bytes: system.total_memory(),
        used_bytes: system.used_memory(),
        available_bytes: system.available_memory(),
        total_swap_bytes: system.total_swap(),
        used_swap_bytes: system.used_swap(),
    };

    let compute = ComputeInfo {
        physical_cores: System::physical_core_count(),
        logical_cpus: system.cpus().len(),
        global_cpu_usage_percent: system.global_cpu_usage(),
        cpus: system
            .cpus()
            .iter()
            .map(|cpu| CpuInfo {
                name: cpu.name().to_string(),
                brand: cpu.brand().to_string(),
                frequency_mhz: cpu.frequency(),
                usage_percent: cpu.cpu_usage(),
            })
            .collect(),
    };

    // Rules are the bridge from facts to decisions: they add severity,
    // evidence, recommendations, and whether approval is required.
    let findings = evaluate_rules(RuleContext {
        memory: &memory,
        compute: &compute,
        storage: &storage,
        ports: &listening_ports,
        kubernetes: &kubernetes,
        gitlab: &gitlab,
        top_processes: &top_processes,
    });

    Snapshot {
        generated_at: Utc::now().to_rfc3339(),
        host: HostInfo {
            hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
            uptime_seconds: System::uptime(),
            boot_time_unix: System::boot_time(),
        },
        os: OsInfo {
            name: System::name().unwrap_or_else(|| "unknown".to_string()),
            long_version: System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
            kernel_version: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
            kernel_long_version: System::kernel_long_version(),
            distribution_id: System::distribution_id(),
            distribution_id_like: System::distribution_id_like(),
            cpu_arch: System::cpu_arch(),
        },
        compute,
        memory,
        storage,
        network,
        listening_ports,
        top_processes,
        service_hints,
        app_hints,
        database_hints,
        kubernetes,
        gitlab,
        findings,
    }
}

fn collect_disks(disks: &Disks) -> Vec<DiskInfo> {
    disks
        .iter()
        .map(|disk| {
            let total = disk.total_space();
            let available = disk.available_space();
            let used_percent = if total == 0 {
                0.0
            } else {
                ((total - available) as f64 / total as f64) * 100.0
            };

            DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point: disk.mount_point().display().to_string(),
                file_system: disk.file_system().to_string_lossy().to_string(),
                kind: format!("{:?}", disk.kind()),
                total_bytes: total,
                available_bytes: available,
                used_percent,
            }
        })
        .collect()
}

fn collect_networks(networks: &Networks) -> Vec<NetworkInfo> {
    networks
        .iter()
        .map(|(interface, data)| NetworkInfo {
            interface: interface.to_string(),
            operational_state: format!("{:?}", data.operational_state()),
            mac_address: data.mac_address().to_string(),
            total_received_bytes: data.total_received(),
            total_transmitted_bytes: data.total_transmitted(),
        })
        .collect()
}

fn collect_top_processes(system: &System) -> Vec<ProcessInfo> {
    let mut processes: Vec<ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.to_string(),
            name: process.name().to_string_lossy().to_string(),
            status: format!("{:?}", process.status()),
            cpu_usage_percent: process.cpu_usage(),
            memory_bytes: process.memory(),
        })
        .collect();

    processes.sort_by(|a, b| {
        b.memory_bytes
            .cmp(&a.memory_bytes)
            .then_with(|| {
                b.cpu_usage_percent
                    .partial_cmp(&a.cpu_usage_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    processes.truncate(25);
    processes
}

fn process_name_set(processes: &[ProcessInfo]) -> BTreeSet<String> {
    processes.iter().map(|p| p.name.to_lowercase()).collect()
}

fn collect_service_hints(processes: &BTreeSet<String>) -> Vec<ServiceHint> {
    let candidates = [
        "sshd",
        "systemd",
        "nginx",
        "httpd",
        "docker",
        "containerd",
        "podman",
        "kubelet",
        "postgres",
        "mysqld",
        "mariadbd",
        "redis",
        "valkey",
        "minio",
        "gitlab",
        "gitlab-workhorse",
        "gitaly",
        "prometheus",
        "grafana",
    ];

    candidates
        .iter()
        .filter(|name| contains_process(processes, name))
        .map(|name| ServiceHint {
            name: name.to_string(),
            source: "process table".to_string(),
            confidence: "medium".to_string(),
        })
        .collect()
}

fn collect_app_hints(processes: &BTreeSet<String>) -> Vec<AppHint> {
    let candidates = [
        "nginx",
        "httpd",
        "node",
        "java",
        "trino",
        "hive",
        "minio",
        "gitlab",
        "prometheus",
        "grafana",
        "docker",
        "containerd",
        "podman",
    ];

    candidates
        .iter()
        .filter(|name| contains_process(processes, name))
        .map(|name| AppHint {
            name: name.to_string(),
            detected_by: "process table".to_string(),
            confidence: "medium".to_string(),
        })
        .collect()
}

fn collect_database_hints(
    processes: &BTreeSet<String>,
    ports: &[super::models::ListeningPort],
) -> Vec<AppHint> {
    let mut hints = Vec::new();

    let db_processes = [
        ("postgresql", "postgres"),
        ("mysql/mariadb", "mysqld"),
        ("mariadb", "mariadbd"),
        ("redis", "redis"),
        ("valkey", "valkey"),
        ("mongodb", "mongod"),
    ];

    for (friendly, process_name) in db_processes {
        if contains_process(processes, process_name) {
            hints.push(AppHint {
                name: friendly.to_string(),
                detected_by: format!("process name contains {process_name}"),
                confidence: "medium".to_string(),
            });
        }
    }

    let known_db_ports = [
        (5432, "postgresql"),
        (3306, "mysql/mariadb"),
        (6379, "redis/valkey"),
        (27017, "mongodb"),
        (9200, "elasticsearch/opensearch"),
    ];

    for (port, name) in known_db_ports {
        if ports.iter().any(|p| p.port == port) {
            hints.push(AppHint {
                name: name.to_string(),
                detected_by: format!("listening port {port}"),
                confidence: "low".to_string(),
            });
        }
    }

    hints.sort_by(|a, b| a.name.cmp(&b.name).then(a.detected_by.cmp(&b.detected_by)));
    hints.dedup_by(|a, b| a.name == b.name && a.detected_by == b.detected_by);

    hints
}

fn contains_process(processes: &BTreeSet<String>, needle: &str) -> bool {
    processes.iter().any(|name| name.contains(needle))
}
