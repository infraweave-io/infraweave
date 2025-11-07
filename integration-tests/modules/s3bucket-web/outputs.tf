output "website_endpoint" {
  value = aws_s3_bucket_website_configuration.this.website_endpoint
}

output "zone_id" {
  value = aws_s3_bucket.this.hosted_zone_id
}

output "evaluate_target_health" {
  value = false
}
