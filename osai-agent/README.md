# OSAI Agent — Quick Start

OSAI Agent is a Rust-first Linux operations assistant.

This guide contains only the required steps to:

- install OSAI directly on an existing Linux machine
- deploy OSAI on Google Cloud using OpenTofu

---

## 1. Required Cognee credentials

Keep these values ready:

```text
COGNEE_API_URL_SECRET
COGNEE_API_KEY_SECRET
COGNEE_TENANT_ID_SECRET
COGNEE_USER_ID_SECRET
OSAI_AGENT_TOKEN_SECRET
```

Generate a new OSAI token:

```bash
echo "OSAI_AGENT_TOKEN_SECRET='$(openssl rand -hex 32)'"
```

Example output:

```bash
OSAI_AGENT_TOKEN_SECRET='4fa17e53c170998e7d2ca3cc44e8b7eaa2fdd9e73717a1631eb58a329fe29f0d'
```

Copy the complete output and keep it safe. The same token is required when accessing the OSAI dashboard.

Do not commit real credentials to a public Git repository.

---

# Option 1 — Install without Google Cloud

## 2. Clone the repository

```bash
git clone https://github.com/Maninder1220/OS.rs.git
cd OS.rs/get-osai-os-ready
```

## 3. Add credentials to the installer

Open the installer:

```bash
vi lets-rust-now.sh
```

Find the credential section and replace the example values:

```bash
set +x

COGNEE_API_URL_SECRET='https://your-cognee-tenant-url.aws.cognee.ai'
COGNEE_API_KEY_SECRET='your-cognee-api-key'
COGNEE_TENANT_ID_SECRET='your-cognee-tenant-id'
COGNEE_USER_ID_SECRET='your-cognee-user-id'
OSAI_AGENT_TOKEN_SECRET='your-generated-osai-token'
```

Do not add `set -x` after the credentials because it enables shell command tracing and may print secrets in the terminal or logs.

Save and exit Vim:

```text
Press Esc
Type :wq
Press Enter
```

## 4. Run the installer

```bash
chmod +x lets-rust-now.sh
sudo bash lets-rust-now.sh
```

## 5. Check OSAI status

```bash
sudo systemctl status osai-agent.service --no-pager
```

If the service is not running:

```bash
sudo systemctl enable --now osai-agent.service
```

View live logs:

```bash
sudo journalctl \
  -fu osai-agent.service \
  -b \
  --no-pager
```

Open the dashboard:

```text
http://127.0.0.1:8000
```

Use the same value configured as:

```text
OSAI_AGENT_TOKEN_SECRET
```

---

# Option 2 — Deploy with Google Cloud and OpenTofu

This assumes Google Cloud CLI and OpenTofu are already installed.

## 6. Authenticate Google Cloud

```bash
gcloud auth login
gcloud auth application-default login
gcloud config set project YOUR_PROJECT_ID
gcloud services enable compute.googleapis.com
```

## 7. Clone the repository

```bash
git clone https://github.com/Maninder1220/OS.rs.git
cd OS.rs
```

## 8. Add credentials to the GCP startup script

Open the startup script:

```bash
cd infra/environments/dev/scripts
vi starters.sh
```

Find the credential section and replace the example values:

```bash
set +x

COGNEE_API_URL_SECRET='https://your-cognee-tenant-url.aws.cognee.ai'
COGNEE_API_KEY_SECRET='your-cognee-api-key'
COGNEE_TENANT_ID_SECRET='your-cognee-tenant-id'
COGNEE_USER_ID_SECRET='your-cognee-user-id'
OSAI_AGENT_TOKEN_SECRET='your-generated-osai-token'
```

Save and exit:

```text
Press Esc
Type :wq
Press Enter
```

The `starters.sh` script is passed to the VM as a Google Cloud startup script.

It runs automatically when the machine is created and starts during the VM boot process. Do not run it manually on your local machine after `tofu apply`.

## 9. Create `terraform.tfvars`

Go to the environment directory:

```bash
cd ..
```

You should now be in:

```text
OS.rs/infra/environments/dev
```

Create the variables file:

```bash
vi terraform.tfvars
```

Add:

```hcl
project_id        = "your-project-id"
region            = "us-central1"
zone              = "us-central1-a"
admin_principal   = "user:your-email@gmail.com"

instance_name     = "yourname-dev-vm"
machine_type      = "e2-standard-2"

boot_disk_type    = "pd-standard"
boot_disk_size_gb = 30

enable_public_ip  = true
```

## 10. Create the VM

```bash
tofu init
tofu fmt
tofu validate
tofu plan -out=tfplan
tofu apply tfplan
```

Show the created outputs:

```bash
tofu output
```

## 11. Connect to the VM

```bash
gcloud compute ssh yourname-dev-vm \
  --zone us-central1-a
```

## 12. Check startup-script logs

Google Cloud runs `starters.sh` automatically.

Check its logs:

```bash
sudo journalctl \
  -u google-startup-scripts.service \
  -b \
  --no-pager
```

Follow the startup script live:

```bash
sudo journalctl \
  -fu google-startup-scripts.service \
  -b \
  --no-pager
```

## 13. Check OSAI service

```bash
sudo systemctl status osai-agent.service --no-pager
```

Start it if required:

```bash
sudo systemctl enable --now osai-agent.service
```

Follow OSAI logs:

```bash
sudo journalctl \
  -fu osai-agent.service \
  -b \
  --no-pager
```

---

# Access services through an SSH tunnel

Run the following command from your local machine:

```bash
gcloud compute ssh yourname-dev-vm \
  --zone us-central1-a \
  -- \
  -L 8000:127.0.0.1:8000 \
  -L 8001:127.0.0.1:8001 \
  -L 8080:127.0.0.1:8080 \
  -L 9000:127.0.0.1:9000 \
  -L 9001:127.0.0.1:9001 \
  -L 5432:127.0.0.1:5432
```

Keep that terminal open while using the services.

Local service addresses:

```text
OSAI dashboard:
http://127.0.0.1:8000

Cognee API:
http://127.0.0.1:8001

llama.cpp / Qwen API:
http://127.0.0.1:8080

RustFS S3 API:
http://127.0.0.1:9000

RustFS console:
http://127.0.0.1:9001

PostgreSQL:
127.0.0.1:5432
```

Use the same OSAI token configured in `starters.sh` when opening the dashboard.

---

# Important commands

Check all running OSAI-related services:

```bash
sudo systemctl status osai-agent.service --no-pager
docker ps
```

Restart OSAI:

```bash
sudo systemctl restart osai-agent.service
```

View the last 100 OSAI log lines:

```bash
sudo journalctl \
  -u osai-agent.service \
  -b \
  -n 100 \
  --no-pager
```

Check listening ports:

```bash
sudo ss -lntp
```
