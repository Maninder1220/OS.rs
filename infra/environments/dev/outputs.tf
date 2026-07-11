output "instance_name" {
  value = module.compute_vm.instance_name
}

output "internal_ip" {
  value = module.compute_vm.internal_ip
}

output "external_ip" {
  value = module.compute_vm.external_ip
}

output "ssh_command" {
  value = "gcloud compute ssh ${module.compute_vm.instance_name} --zone ${var.zone} --tunnel-through-iap"
}