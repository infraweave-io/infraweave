from infraweave import S3Bucket, Deployment

s3bucket = S3Bucket(
    version='0.0.36-dev+test.33', 
    track="dev"
)

# bc = BucketCollection(
#     version='0.0.18-dev+test.1',
#     track='dev'
# )

print(s3bucket.get_name())
# print(bc.get_name())

# bucketcollection1 = Deployment(
#     name="bucketcollection1",
#     environment="dev",
#     stack=bc,
#     region="us-west-2",
# )

bucket1 = Deployment(
    name="bucket1",
    namespace="dev",
    region="us-west-2",
    module=s3bucket,
)

with bucket1:

    print(bucket1.outputs)

    bucket1.set_variables(
        bucket_name="my-bucket12347ydfs3",
        enable_acl=False,
    )
    bucket1.apply()
