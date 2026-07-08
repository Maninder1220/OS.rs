// =============================================================================
// File: src/collector/models.rs
// Purpose:
//   Typed snapshot model shared by scanner, rules, API responses, storage worker, and UI.
//
// Where this fits in OSAI:
//   Defines the data contract for what OSAI knows about a host at scan time.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Changing these structs can affect JSON history, database JSONB, rules, and frontend rendering.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub generated_at: String,
    pub host: HostInfo,
    pub os: OsInfo,
    pub compute: ComputeInfo,
    pub memory: MemoryInfo,
    pub storage: Vec<DiskInfo>,
    pub network: Vec<NetworkInfo>,
    pub listening_ports: Vec<ListeningPort>,
    pub top_processes: Vec<ProcessInfo>,
    pub service_hints: Vec<ServiceHint>,
    pub app_hints: Vec<AppHint>,
    pub database_hints: Vec<AppHint>,
    pub kubernetes: KubernetesHint,
    pub gitlab: GitlabHint,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub uptime_seconds: u64,
    pub boot_time_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub long_version: String,
    pub kernel_version: String,
    pub kernel_long_version: String,
    pub distribution_id: String,
    pub distribution_id_like: Vec<String>,
    pub cpu_arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeInfo {
    pub physical_cores: Option<usize>,
    pub logical_cpus: usize,
    pub global_cpu_usage_percent: f32,
    pub cpus: Vec<CpuInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub name: String,
    pub brand: String,
    pub frequency_mhz: u64,
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub total_swap_bytes: u64,
    pub used_swap_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub kind: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub interface: String,
    pub operational_state: String,
    pub mac_address: String,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListeningPort {
    pub protocol: String,
    pub local_address_raw: String,
    pub port: u16,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: String,
    pub name: String,
    pub status: String,
    pub cpu_usage_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHint {
    pub name: String,
    pub source: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppHint {
    pub name: String,
    pub detected_by: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesHint {
    pub detected: bool,
    pub available: bool,
    pub summary: String,
    pub signals: Vec<String>,
    pub safe_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabHint {
    pub detected: bool,
    pub available: bool,
    pub summary: String,
    pub signals: Vec<String>,
    pub safe_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub detail: String,
    pub evidence: Vec<String>,
    pub recommendation: String,
    pub requires_approval: bool,
    pub command_suggestion: Option<String>,
    pub plugin: Option<String>,
}
