terraform {
  required_version = ">= 1.10, < 2.0"
}

# Interfaz neutra: registra la especificación, NO contrata una máquina.
variable "spec" {
  type = object({
    name     = string
    role     = string
    region   = string
    image    = string
    size     = string
    ssh_keys = list(string)
    ipv4     = string
    ipv6     = string
  })
  validation {
    condition     = contains(["principal", "relay"], var.spec.role) && length(var.spec.ssh_keys) > 0 && can(cidrhost("${var.spec.ipv4}/32", 0)) && can(cidrhost("${var.spec.ipv6}/128", 0))
    error_message = "Se requiere rol principal/relay, clave SSH e IPv4/IPv6 válidas."
  }
}

resource "terraform_data" "server" {
  input = var.spec
}

output "server" {
  value = terraform_data.server.output
}
