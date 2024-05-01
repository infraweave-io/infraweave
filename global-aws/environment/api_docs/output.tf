
output "function_arn" {
  value = aws_lambda_function.api_docs.arn
}

output "docs_s3_bucket_name" {
  value = aws_s3_bucket.docs_bucket.bucket
}

output "docs_s3_bucket_arn" {
  value = aws_s3_bucket.docs_bucket.arn
}

output "ecr_repository_url" {
  value = aws_ecr_repository.docs_lambda_repository.repository_url
}
