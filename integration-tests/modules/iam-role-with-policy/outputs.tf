output "role_arn" {
  value       = aws_iam_role.main.arn
  description = "ARN of the IAM role"
}

output "role_name" {
  value       = aws_iam_role.main.name
  description = "Name of the IAM role"
}
