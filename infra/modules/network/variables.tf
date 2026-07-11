variable "network_name" { type = string }
variable "region" { type = string }
variable "subnet_cidr" { type = string }

variable "enable_iap_ssh" {
  type    = bool
  default = true
}

variable "enable_public_ip" {
  type    = bool
  default = false
}

variable "admin_source_cidr" {
  type    = string
  default = "0.0.0.0/32"
}