variable "project_id" { type = string }
variable "region" { type = string }
variable "zone" { type = string }
variable "admin_principal" { type = string }

variable "instance_name" {
  type    = string
  default = "alma-dev-vm"
}

variable "machine_type" {
  type    = string
  default = "e2-micro"
}

variable "boot_image" {
  type    = string
  default = "projects/almalinux-cloud/global/images/family/almalinux-9"
}

variable "boot_disk_type" {
  type    = string
  default = "pd-standard"
}

variable "boot_disk_size_gb" {
  type    = number
  default = 30
}

variable "enable_public_ip" {
  type    = bool
  default = false
}

variable "admin_source_cidr" {
  type    = string
  default = "0.0.0.0/32"
}

variable "enable_iap_ssh" {
  type    = bool
  default = true
}