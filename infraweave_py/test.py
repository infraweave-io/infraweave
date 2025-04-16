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
    environment="dev",
    region="us-west-2",
    module=s3bucket,
)

print(bucket1.outputs)

bucket1.set_variables(
    bucket_name="my-bucket12347ydfs3",
    enable_acl=False,
)

try:
    bucket1.apply()
    print(bucket1.outputs)
    # Run some tests here
except Exception as e:
    print(f"An error occurred: {e}")
    # Handle the error as needed
finally:
    bucket1.destroy()
