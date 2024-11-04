from infraweave_py import Module, Deployment

module_bucket = Module(
    name="S3Bucket", 
    version='0.0.36-dev+test.6', 
    track="dev"
)

bucket1 = Deployment(
    name="bucket1",
    environment="dev",
    module=module_bucket,
)

variables = {
    "bucket_name": "my-bucket12347ydfs3",
    "enable_acl": False
}

bucket1.set_variables(variables)
bucket1.apply()

# Run some tests here

variables["enable_acl"] = True
bucket1.set_variables(variables)
bucket1.apply()

# Run some tests here

bucket1.destroy()
