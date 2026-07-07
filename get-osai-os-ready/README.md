# OSAI OS Readiness Kit

This kit prepares a fresh Ubuntu, Red Hat, or AlmaLinux server so the OSAI app stack can fit in cleanly.

It was prepared from the attached architecture resources:

- OSAI uses Rust as the scanner/API/control plane.
- Docker Compose runs supporting services.
- Postgres stores structured facts and metadata.
- RustFS or MinIO-style object storage stores raw JSON, Markdown memory, logs, and evidence.
- Cognee handles memory and retrieval.
- llama.cpp runs the local Qwen GGUF model.
- OpenTelemetry sends traces, metrics, and logs toward SigNoz.

## Files

- `bootstrap-osai-host.sh` - install, status, and doctor script.
- `README.md` - this operator guide.

## Recommended First Run

For a normal app-ready host:

```bash
sudo bash bootstrap-osai-host.sh install
```

For a host where you will compile Rust on the server:

```bash
sudo bash bootstrap-osai-host.sh install --with-rust
```

For a host that should also run SigNoz locally:

```bash
sudo bash bootstrap-osai-host.sh install --with-signoz
```

For a public/lab VM where you intentionally want local app ports opened:

```bash
sudo bash bootstrap-osai-host.sh install --open-local-firewall
```

The safer default is to keep ports private and use SSH/IAP tunnels.

## End Status Check

Run:

```bash
sudo bash bootstrap-osai-host.sh status
```

Expected important checks:

- Docker service is active.
- Docker Compose v2 is available.
- `/opt/osai/config/osai.env` exists.
- `/opt/osai/models` exists for the GGUF model.
- OTel Collector service is active if OTel was not disabled.

For deeper troubleshooting:

```bash
sudo bash bootstrap-osai-host.sh doctor
```

## What The Script Installs

Base packages:

- `curl`, `git`, `tar`, `unzip`, `jq`
- compiler/build tools
- OpenSSL development headers
- networking/process inspection tools
- `firewalld` on Red Hat/AlmaLinux or `ufw` on Ubuntu

Runtime:

- Docker Engine
- Docker Compose v2 plugin
- optional Rust toolchain through rustup
- optional SigNoz using Foundry
- optional lightweight OpenTelemetry Collector

## Directory Layout

The script creates:

```text
/opt/osai/
  app/
  config/osai.env
  data/
  logs/
  models/
  otel/
  signoz/
```

Put the local Qwen model here:

```text
/opt/osai/models/Qwen3-4B-Q4_K_M.gguf
```

## Default Local Ports

| Port | Service |
|---:|---|
| 8000 | OSAI Rust API/dashboard |
| 8001 | Cognee REST API |
| 8080 | llama.cpp or SigNoz UI, depending on your compose setup |
| 9000 | RustFS/MinIO S3-compatible API |
| 9001 | RustFS/MinIO console |
| 14317 | local OTel gRPC receiver |
| 14318 | local OTel HTTP receiver |
| 13133 | local OTel health endpoint |
| 4317 | SigNoz OTLP gRPC ingest |
| 4318 | SigNoz OTLP HTTP ingest |

Avoid exposing `4317`, `4318`, model ports, database ports, or object storage ports publicly.

## Browser Error You Pasted

The error:

```text
QuotaExceededError: Failed to execute 'setItem' on 'Storage'
```

is a browser-side ChatGPT local storage/cache issue. It means the browser storage quota for `chatgpt.com` is full. It is not an OSAI server error.

Fix on Chrome/Edge:

1. Open `chrome://settings/siteData`.
2. Search `chatgpt.com`.
3. Delete site data for ChatGPT.
4. Reload ChatGPT and sign in again if asked.

The preload warning for `page-table-row-...css` is also browser-side and usually harmless.

## Production Notes

For GCP or any cloud VM, prefer:

- private VM by default
- outbound NAT for package/model downloads
- SSH tunnel or IAP tunnel for UI access
- no public firewall for SigNoz ingest ports
- pinned Docker image versions once the demo is stable
- pre-baked image later, instead of installing everything on each boot

For OSAI, the good production shape is:

1. Bash/OpenTofu prepares the OS and VM.
2. Docker Compose starts Postgres, RustFS, Cognee, llama.cpp, and optional SigNoz.
3. Rust binary runs the agent/scanner/API.
4. OTel/SigNoz show traces, metrics, logs, and AI workflow latency.
