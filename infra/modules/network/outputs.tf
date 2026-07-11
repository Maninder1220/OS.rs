output "network_id" {
  value = google_compute_network.gcp_network.id
}

output "subnetwork_id" {
  value = google_compute_subnetwork.gcp_subnet.id
}