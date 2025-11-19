variable "bucket_name" {
  type        = string
  description = "Name of the S3 bucket"
}

variable "policy" {
  type        = string
  description = "Bucket policy JSON"
}
