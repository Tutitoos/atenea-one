# EJEMPLO de adaptador concreto. No ejecutar sin autorización del responsable DNS.
terraform {
  required_version = ">= 1.10, < 2.0"
  required_providers {
    dns = {
      source  = "hashicorp/dns"
      version = "3.4.3"
    }
  }
}

variable "dns_server" {
  type = string
}
variable "dns_key_name" {
  type = string
}
variable "dns_key_secret" {
  type      = string
  sensitive = true
}
variable "zone" {
  type    = string
  default = "example.test."
}
variable "address" {
  type    = string
  default = "192.0.2.10"
}

provider "dns" {
  update {
    server        = var.dns_server
    key_name      = var.dns_key_name
    key_algorithm = "hmac-sha256"
    key_secret    = var.dns_key_secret
  }
}

resource "dns_a_record_set" "panel" {
  zone      = var.zone
  name      = "panel"
  addresses = [var.address]
  ttl       = 300
}

output "fqdn" {
  value = dns_a_record_set.panel.name
}
