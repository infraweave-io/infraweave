package infraweave.terraform_plan

deny[msg] {
  resource := input.resource_changes[_]
  resource.mode == "managed"
  resource.type == "aws_s3_bucket"
  resource.change.actions != ["delete"]

  bucket_name := resource.change.after.bucket
  required_prefix := data.requiredPrefix
  not startswith(bucket_name, required_prefix)

  msg := sprintf("S3 bucket %s must start with prefix %q", [resource.address, required_prefix])
}
