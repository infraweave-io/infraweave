
# Check AWS Credentials validity

# Push a new module manifest to the registry

# Check if the module exists in the registry

# Check if cluster exists
CLUSTER_NAME=test1
kind get clusters | grep $CLUSTER_NAME && echo "Cluster exists" && kind delete cluster --name=$CLUSTER_NAME || echo "Cluster does not exist"

kind create cluster --name=$CLUSTER_NAME

kind load docker-image infrabridge-operator:latest --name $CLUSTER_NAME

helm upgrade -i infrabridge-operator ../infrabridge-helm \
  --set aws.accessKeyId=$AWS_ACCESS_KEY_ID \
  --set aws.secretAccessKey=$AWS_SECRET_ACCESS_KEY \
  --set aws.sessionToken=$AWS_SESSION_TOKEN \
  --set aws.region=eu-central-1

sleep 5 # TODO: Remove this line when operator-check is added below

# Wait until operator is ready

# Check if S3Bucket CRD exists

kubectl get s3bucket && echo "S3Bucket CRD exists" || (echo "S3Bucket CRD does not exist" && exit 1)

NAME=my-s3-bucket
BUCKET=my-unique-bucket-name-3543tea

kubectl apply -f -<<EOF
apiVersion: infrabridge.io/v1
kind: S3Bucket
metadata:
  name: $NAME
  namespace: default
spec:
  bucketName: $BUCKET
  region: eu-central-1
EOF

# Wait until status is ready
STATUS=$(kubectl get s3bucket $NAME -o jsonpath='{.status.resourceStatus}')
while [ "$STATUS" != "apply: finished" ]; do
  echo "Waiting for S3Bucket to be ready, current status: $STATUS"
  sleep 5
  STATUS=$(kubectl get s3bucket $NAME -o jsonpath='{.status.resourceStatus}')
done

aws s3 ls s3://$BUCKET && echo "$BUCKET exists in AWS" || echo "$BUCKET was not created properly"

kubectl delete s3bucket $NAME --wait=false

# Wail until bucket is removed in AWS, check return code
# aws s3 ls s3://$BUCKET
EXISTS=$(aws s3 ls s3://$BUCKET 2>&1)
STATUS=$(kubectl get s3bucket $NAME -o jsonpath='{.status.resourceStatus}')
while [ "$EXISTS" == "" ]; do
  echo "Waiting for S3Bucket to be removed, current status: $STATUS"
  sleep 5
  EXISTS=$(aws s3 ls s3://$BUCKET 2>&1)
  STATUS=$(kubectl get s3bucket $NAME -o jsonpath='{.status.resourceStatus}')
done

# Ensure s3bucket is removed
kubectl get s3bucket $NAME && echo "$NAME was not removed properly" && exit 1 || echo "$NAME was removed properly"

# kind delete cluster --name=$CLUSTER_NAME
