// =============================================================================
// File: src/collector/ports.rs
// Purpose:
//   Reads listening network ports from Linux procfs-style socket tables.
//
// Where this fits in OSAI:
//   Feeds scanner port facts and rule checks for exposed services.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Parsing must be defensive because procfs rows vary by protocol and kernel behavior.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use super::models::ListeningPort;

pub fn collect_listening_ports() -> Vec<ListeningPort> {
    let mut ports = Vec::new();

    ports.extend(parse_proc_net("/proc/net/tcp", "tcp"));
    ports.extend(parse_proc_net("/proc/net/tcp6", "tcp6"));
    ports.extend(parse_proc_net("/proc/net/udp", "udp"));
    ports.extend(parse_proc_net("/proc/net/udp6", "udp6"));

    ports.sort_by(|a, b| a.port.cmp(&b.port).then(a.protocol.cmp(&b.protocol)));
    ports.dedup_by(|a, b| {
        a.protocol == b.protocol && a.port == b.port && a.local_address_raw == b.local_address_raw
    });

    ports
}

fn parse_proc_net(path: &str, protocol: &str) -> Vec<ListeningPort> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    for line in content.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();

        if columns.len() < 4 {
            continue;
        }

        let local_address = columns[1];
        let state_hex = columns[3];

        let Some((addr_raw, port_hex)) = local_address.split_once(':') else {
            continue;
        };

        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };

        // /proc/net stores ports in hex and TCP states as numeric hex codes.
        // Convert them here so API clients and rules do not need kernel-table knowledge.
        let state = socket_state(state_hex);

        if protocol.starts_with("tcp") && state != "LISTEN" {
            continue;
        }

        out.push(ListeningPort {
            protocol: protocol.to_string(),
            local_address_raw: addr_raw.to_string(),
            port,
            state: state.to_string(),
        });
    }

    out
}

fn socket_state(hex: &str) -> &'static str {
    match hex {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}
