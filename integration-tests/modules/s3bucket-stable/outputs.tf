output "bucket_arn" {
  value = aws_s3_bucket.example.arn
}

output "bucket_name" {
  value = aws_s3_bucket.example.bucket
}

output "sse_algorithm" {
  value = aws_s3_bucket.example.bucket_domain_name
}
