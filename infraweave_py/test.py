from infraweave import S3Bucket, Deployment

s3bucket = S3Bucket(
    version='0.0.36-dev+test.33', 
    track="dev"
)

# bc = BucketCollection(
#     version='0.0.18-dev+test.1',
#     track='dev'
# )

print(s3bucket)
# print(bc)

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
    res = bucket1.apply()
    print(res)
    print(res.get_output())

    # Only change the bucket name, keep other settings same
    bucket1.set_variables(bucket_name="my-bucket12347ydfs4")
    res = bucket1.plan()
    print(res)
    print(f"is destructive: {res.has_destructive_changes()}")
    if res.has_destructive_changes():
        print("Destructive changes detected:")
        for address, action in res.get_destructive_changes():
            print(f"  - {action}: {address}")
    print(res.get_output())