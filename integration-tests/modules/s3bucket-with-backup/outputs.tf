output "bucket_arn" {
  value = aws_s3_bucket.primary.arn
}

output "region" {
  value = aws_s3_bucket.primary.region
}

output "sse_algorithm" {
  value = aws_s3_bucket.primary.bucket_domain_name
}


output "backup_bucket_arn" {
  value = aws_s3_bucket.backup.arn
}

output "backup_region" {
  value = aws_s3_bucket.backup.region
}

output "backup_sse_algorithm" {
  value = aws_s3_bucket.backup.bucket_domain_name
}