package infraweave.terraform_plan

deny[msg] {
  resource := input.resource_changes[_]
  resource.mode == "managed"
  resource.type == "aws_s3_bucket"
  resource.change.actions != ["delete"]

  bucket_name := resource.change.after.bucket
  bucket_name == data.blockedBucketName

  msg := sprintf("S3 bucket name %q is blocked by policy", [bucket_name])
}
