variable "modules" {
  type    = map(object({
    name = string
    repo = string
  }))
}