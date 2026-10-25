terraform {
  required_version = ">= 1.10, < 2.0"
}

# Interfaz neutra sin efectos DNS. El adaptador elegido consume esta salida.
variable "records" {
  type = map(object({
    name  = string
    type  = string
    value = string
    ttl   = number
  }))
  validation {
    condition     = alltrue([for r in values(var.records) : contains(["A", "AAAA", "CNAME"], r.type) && r.ttl >= 60 && r.ttl == floor(r.ttl) && length(r.name) > 0 && length(r.value) > 0])
    error_message = "Registros A/AAAA/CNAME con nombre, valor y TTL entero >= 60."
  }
}

resource "terraform_data" "records" {
  input = var.records
}

output "records" {
  value = terraform_data.records.output
}
