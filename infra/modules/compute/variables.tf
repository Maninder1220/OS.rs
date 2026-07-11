variable "instance_name" { type = string }
variable "machine_type" { type = string }
variable "zone" { type = string }
variable "boot_image" { type = string }
variable "boot_disk_type" { type = string }
variable "boot_disk_size_gb" { type = number }
variable "subnetwork_id" { type = string }
variable "service_account" { type = string }
variable "enable_public_ip" { type = bool }

variable "startup_script_path" {
  description = "Local path to startup script file. Leave empty to disable startup script."
  type        = string
  default     = ""
}