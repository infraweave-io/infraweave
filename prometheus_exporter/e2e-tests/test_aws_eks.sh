
# Check AWS Credentials validity

# Push a new module manifest to the registry

# Check if the module exists in the registry

# docker build -t prometheus-exporter:latest -f prometheus_exporter/Dockerfile . --load

# Check if cluster exists
CLUSTER_NAME=test1
kind get clusters | grep $CLUSTER_NAME && echo "Cluster exists" && kind delete cluster --name=$CLUSTER_NAME || echo "Cluster does not exist"

kind create cluster --name=$CLUSTER_NAME


helm repo add prometheus-community https://prometheus-community.github.io/helm-charts && \
helm repo add grafana https://grafana.github.io/helm-charts && \
helm repo update && \
helm upgrade -i prometheus prometheus-community/prometheus -f prometheus_exporter/e2e-tests/prometheus-values.yaml && \
helm upgrade -i grafana grafana/grafana -f prometheus_exporter/e2e-tests/grafana-values.yaml


kind load docker-image prometheus-exporter:latest --name $CLUSTER_NAME

helm upgrade -i infraweave-prometheus-exporter ./prometheus_exporter/helm \
  --set aws.accessKeyId=$AWS_ACCESS_KEY_ID \
  --set aws.secretAccessKey=$AWS_SECRET_ACCESS_KEY \
  --set aws.sessionToken=$AWS_SESSION_TOKEN \
  --set aws.region=eu-central-1

sleep 5 # TODO: Remove this line when operator-check is added below

kubectl wait --for=condition=Ready pod -l app.kubernetes.io/name=grafana --timeout=300s

kubectl port-forward svc/grafana 9003:80
