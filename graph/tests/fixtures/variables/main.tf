variable "content" {
  description = "The content of the file"
  type        = string
  default     = "default content"
}

variable "filename" {
  description = "The name of the file"
  type        = string
  default     = "file.txt"
}

resource "local_file" "file" {
  content  = var.content
  filename = "${path.module}/${var.filename}"
}
