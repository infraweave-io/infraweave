from infraweave_py import S3Bucket, Deployment, BucketCollection

s3bucket = S3Bucket(
    version='0.0.36-dev+test.6', 
    track="dev"
)

bc = BucketCollection(
    version='0.0.14-dev+test.1',
    track='dev'
)

print(s3bucket.get_name())
print(bc.get_name())

# bucketcollection1 = Deployment(
#     name="bucketcollection1",
#     environment="dev",
#     stack=bc
# )

bucket1 = Deployment(
    name="bucket1",
    environment="dev",
    module=s3bucket,
)

bucket1.set_variables(
    bucket_name="my-bucket12347ydfs3",
    enable_acl=False
)
bucket1.apply()

# Run some tests here

bucket1.set_variables(
    bucket_name="my-bucket12347ydfs3",
    enable_acl=True
)
bucket1.apply()

# Run some tests here

bucket1.destroy()
