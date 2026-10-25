terraform {
  required_version = ">= 1.10, < 2.0"
  backend "local" {}
}

module "principal" {
  source = "../../modules/server"
  spec = {
    name     = "principal.example.test"
    role     = "principal"
    region   = "laboratorio"
    image    = "debian-13"
    size     = "ejemplo"
    ssh_keys = ["clave-publica-sintetica-no-utilizable"]
    ipv4     = "192.0.2.10"
    ipv6     = "2001:db8::10"
  }
}

module "dns" {
  source = "../../modules/dns_records"
  records = {
    panel4 = { name = "panel.example.test.", type = "A", value = module.principal.server.ipv4, ttl = 300 }
    panel6 = { name = "panel.example.test.", type = "AAAA", value = module.principal.server.ipv6, ttl = 300 }
  }
}

output "inventory" {
  value = module.principal.server
}

output "dns_records" {
  value = module.dns.records
}
