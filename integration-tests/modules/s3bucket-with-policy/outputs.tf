output "bucket_arn" {
  value       = aws_s3_bucket.main.arn
  description = "ARN of the S3 bucket"
}

output "bucket_name" {
  value       = aws_s3_bucket.main.bucket
  description = "Name of the S3 bucket"
}
