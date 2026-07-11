resource "google_service_account" "vm" {
  account_id   = "${var.instance_name}-sa"
  display_name = "Service account for ${var.instance_name}"
}

resource "google_project_iam_member" "vm_log_writer" {
  project = var.project_id
  role    = "roles/logging.logWriter"
  member  = "serviceAccount:${google_service_account.vm.email}"
}

resource "google_project_iam_member" "vm_metric_writer" {
  project = var.project_id
  role    = "roles/monitoring.metricWriter"
  member  = "serviceAccount:${google_service_account.vm.email}"
}

resource "google_project_iam_member" "admin_os_login" {
  project = var.project_id
  role    = "roles/compute.osAdminLogin"
  member  = var.admin_principal
}

resource "google_project_iam_member" "admin_iap_tunnel" {
  project = var.project_id
  role    = "roles/iap.tunnelResourceAccessor"
  member  = var.admin_principal
}