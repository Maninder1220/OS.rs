# OSAI Google Cloud Infrastructure

This OpenTofu configuration creates one AlmaLinux Compute Engine VM and installs OSAI through the VM startup script.

## VM Configuration

```text
Machine family:     E2 general purpose
Machine type:       e2-standard-2
vCPUs:              2
CPU threads:        2 logical hardware threads
Memory:             8 GB
Boot disk:          30 GB pd-standard
Region:             us-central1
Zone:               us-central1-a
Public IP:          Enabled
Operating system:   AlmaLinux
Startup script:     scripts/starters.sh
```

Google Cloud implements each vCPU as one hardware thread. The VM sees two logical CPUs. The underlying physical host cores are not dedicated or exposed to the VM.

---

## 1. Authenticate Google Cloud

```bash
gcloud auth login
gcloud auth application-default login
gcloud config set project YOUR_PROJECT_ID
gcloud services enable compute.googleapis.com
```

---

## 2. Add OSAI Credentials

Generate the dashboard token:

```bash
echo "OSAI_AGENT_TOKEN_SECRET='$(openssl rand -hex 32)'"
```

Open the startup script:

```bash
cd OS.rs/infra/environments/dev/scripts
vi starters.sh
```

Replace the example credential values:

```bash
set +x

COGNEE_API_URL_SECRET='https://your-cognee-tenant-url.aws.cognee.ai'
COGNEE_API_KEY_SECRET='your-cognee-api-key'
COGNEE_TENANT_ID_SECRET='your-cognee-tenant-id'
COGNEE_USER_ID_SECRET='your-cognee-user-id'
OSAI_AGENT_TOKEN_SECRET='your-generated-osai-token'
```

Save and exit Vim:

```text
Press Esc
Type :wq
Press Enter
```

Do not commit real credentials to Git.

---

## 3. Configure the Environment

Go to the OpenTofu environment:

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

Add your values:

```hcl
project_id        = "your-gcp-project-id"
region            = "us-central1"
zone              = "us-central1-a"
admin_principal   = "user:your-email@gmail.com"

instance_name     = "yourname-dev-vm"
machine_type      = "e2-standard-2"

boot_disk_type    = "pd-standard"
boot_disk_size_gb = 30

enable_public_ip  = true
```

`terraform.tfvars` must remain excluded by `.gitignore`.

---

## 4. Enable the VM Startup Script

The startup script input is commented out by default. Open the environment `main.tf` file:

```bash
vi main.tf
```

Find the `module "compute_vm"` block.

Inside that module, locate this commented line:

```hcl
# startup_script_path = "${path.root}/scripts/starters.sh"
```

Uncomment it so it becomes:

```hcl
startup_script_path = "${path.root}/scripts/starters.sh"
```

Example:

```hcl
module "compute_vm" {
  source = "../../modules/compute"

  project_id      = var.project_id
  region          = var.region
  zone            = var.zone
  instance_name   = var.instance_name
  machine_type    = var.machine_type
  network         = module.network.network_self_link
  subnetwork      = module.network.subnetwork_self_link
  boot_disk_type  = var.boot_disk_type
  boot_disk_size_gb = var.boot_disk_size_gb
  enable_public_ip  = var.enable_public_ip

  startup_script_path = "${path.root}/scripts/starters.sh"
}
```

The exact surrounding arguments may differ in your repository. The important requirement is that this line is active inside `module "compute_vm"`:

```hcl
startup_script_path = "${path.root}/scripts/starters.sh"
```

Verify that the setting is no longer commented:

```bash
grep -n 'startup_script_path' main.tf
```

Expected output:

```text
startup_script_path = "${path.root}/scripts/starters.sh"
```

Confirm that the startup script exists:

```bash
test -f scripts/starters.sh \
  && echo "OK: scripts/starters.sh exists" \
  || echo "ERROR: scripts/starters.sh is missing"
```

Format and validate the configuration:

```bash
tofu fmt
tofu validate
```

If `startup_script_path` remains commented, the Compute Engine VM can still be created, but `scripts/starters.sh` will not be passed to the VM and OSAI will not be installed automatically.

---

## 5. Deploy

Run these commands from:

```text
OS.rs/infra/environments/dev
```

Initialize the OpenTofu working directory:

```bash
tofu init
```

Format and validate:

```bash
tofu fmt
tofu validate
```

Create a saved execution plan:

```bash
tofu plan -out=tfplan
```

Apply the saved plan:

```bash
tofu apply tfplan
```

Show the deployed outputs:

```bash
tofu output
```

The VM receives `scripts/starters.sh` as its startup script only when this argument is enabled inside `module "compute_vm"`:

```hcl
startup_script_path = "${path.root}/scripts/starters.sh"
```

The startup script runs automatically when the VM is created. Do not run the local script manually after `tofu apply`.

---

## 6. Connect to the VM

Direct SSH:

```bash
gcloud compute ssh yourname-dev-vm \
  --project=YOUR_PROJECT_ID \
  --zone=us-central1-a
```

SSH through Identity-Aware Proxy:

```bash
gcloud compute ssh yourname-dev-vm \
  --project=YOUR_PROJECT_ID \
  --zone=us-central1-a \
  --tunnel-through-iap
```

Use `--tunnel-through-iap` when the VM has no public IP, direct SSH is blocked, or you intentionally require IAP access.

---

## 7. Check Installation

Check the Google Cloud startup script:

```bash
sudo journalctl \
  -u google-startup-scripts.service \
  -b \
  --no-pager
```

Follow startup-script logs live until you press `Ctrl+C`:

```bash
sudo journalctl \
  -fu google-startup-scripts.service \
  -b \
  --no-pager \
  -n all
```

Follow the dedicated OSAI startup log:

```bash
sudo tail -F /var/log/osai/startup.log
```

Check OSAI:

```bash
sudo systemctl status osai-agent.service --no-pager
```

Start OSAI if it is not running:

```bash
sudo systemctl enable --now osai-agent.service
```

Follow OSAI logs:

```bash
sudo journalctl \
  -fu osai-agent.service \
  -b \
  --no-pager \
  -n all
```

Check the systemd credentials service:

```bash
sudo systemctl status osai-secrets.service --no-pager
```

Check the installation completion marker:

```bash
sudo cat /var/lib/osai/startup.done
```

Verify the VM resources:

```bash
lscpu
free -h
lsblk
```

Expected CPU and memory:

```text
CPU(s): 2
Memory: approximately 8 GB
```

---

## 8. Access OSAI Services

Run the tunnel command from your local machine:

```bash
gcloud compute ssh yourname-dev-vm \
  --project=YOUR_PROJECT_ID \
  --zone=us-central1-a \
  --tunnel-through-iap \
  -- -N \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=30 \
  -L 8000:127.0.0.1:8000 \
  -L 8001:127.0.0.1:8001 \
  -L 8080:127.0.0.1:8080 \
  -L 9000:127.0.0.1:9000 \
  -L 9001:127.0.0.1:9001 \
  -L 5432:127.0.0.1:5432
```

Keep the terminal open.

```text
OSAI dashboard:  http://127.0.0.1:8000
Cognee API:      http://127.0.0.1:8001
Qwen API:        http://127.0.0.1:8080
RustFS API:      http://127.0.0.1:9000
RustFS console:  http://127.0.0.1:9001
PostgreSQL:      127.0.0.1:5432
```

Use the configured `OSAI_AGENT_TOKEN_SECRET` to access the OSAI dashboard.

Stop the SSH tunnel with:

```text
Ctrl+C
```

---

## 9. Destroy the Infrastructure

Review the destroy plan:

```bash
tofu plan -destroy
```

Destroy the VM and related resources:

```bash
tofu destroy
```
