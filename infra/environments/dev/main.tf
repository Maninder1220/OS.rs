module "network" {
  source = "../../modules/network"

  network_name      = "dev-vpc"
  region            = var.region
  subnet_cidr       = "10.10.0.0/24"
  enable_iap_ssh    = var.enable_iap_ssh
  enable_public_ip  = var.enable_public_ip
  admin_source_cidr = var.admin_source_cidr
}

module "iam" {
  source = "../../modules/iam"

  project_id      = var.project_id
  instance_name   = var.instance_name
  admin_principal = var.admin_principal
}

module "compute_vm" {
  source = "../../modules/compute"

  instance_name     = var.instance_name
  machine_type      = var.machine_type
  zone              = var.zone
  boot_image        = var.boot_image
  boot_disk_type    = var.boot_disk_type
  boot_disk_size_gb = var.boot_disk_size_gb
  subnetwork_id     = module.network.subnetwork_id
  service_account   = module.iam.service_account_email
  enable_public_ip  = var.enable_public_ip

  startup_script_path = "${path.root}/scripts/starters.sh"
}