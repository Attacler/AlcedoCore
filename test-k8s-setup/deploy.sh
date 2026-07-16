#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLUSTER_NAME="${1:-plugin-core-test}"
REGISTRY_NAME="k3d-${CLUSTER_NAME}"

echo "=== Creating K3d cluster: ${CLUSTER_NAME} ==="
k3d cluster create "${CLUSTER_NAME}" --config "${SCRIPT_DIR}/k3d.yml" 2>/dev/null || true

# Merge kubeconfig
k3d kubeconfig merge "${CLUSTER_NAME}" -d "${HOME}/.kube/config" 2>/dev/null || true
kubectl config use-context "k3d-${CLUSTER_NAME}" 2>/dev/null || true

echo ""
echo "=== Building and pushing K8s Docker image ==="
cd "${SCRIPT_DIR}/../plugin-core"
cargo build --release -p bins --bin k8s 2>&1 | tail -1
cd "${SCRIPT_DIR}/.."
docker build --no-cache -f plugin-core/Dockerfile.k8s -t "${REGISTRY_NAME}:5000/alcedocore/core:k8s" . 2>&1 | tail -1
docker push "${REGISTRY_NAME}:5000/alcedocore/core:k8s" 2>&1 | tail -2

# Update image references in manifests
sed -i "s|image:.*alcedocore/core:k8s|image: ${REGISTRY_NAME}:5000/alcedocore/core:k8s|g" "${SCRIPT_DIR}/06-core-deployment.yaml"
sed -i 's|imagePullPolicy:.*|imagePullPolicy: Always|g' "${SCRIPT_DIR}/06-core-deployment.yaml"
sed -i "s|LOCAL_REGISTRY_URL:.*|LOCAL_REGISTRY_URL: \"${REGISTRY_NAME}:5000\"|" "${SCRIPT_DIR}/02-configmap.yaml"

echo ""
echo "=== Applying Kubernetes manifests ==="
kubectl apply -f "${SCRIPT_DIR}/00-namespace.yaml"
kubectl apply -f "${SCRIPT_DIR}/01-rbac.yaml"
kubectl apply -f "${SCRIPT_DIR}/02-configmap.yaml"
kubectl apply -f "${SCRIPT_DIR}/03-secrets.yaml"
kubectl apply -f "${SCRIPT_DIR}/04-postgres.yaml"
kubectl apply -f "${SCRIPT_DIR}/05-redis.yaml"

echo ""
echo "=== Waiting for PostgreSQL and Redis ==="
kubectl wait --for=condition=ready pod -l app=postgres -n plugin-core --timeout=120s
kubectl wait --for=condition=ready pod -l app=redis -n plugin-core --timeout=60s

echo ""
echo "=== Deploying metrics-server ==="
kubectl apply -f https://github.com/kubernetes-sigs/metrics-server/releases/latest/download/components.yaml 2>/dev/null || true
kubectl patch deployment metrics-server -n kube-system --type='json' \
  -p='[{"op": "add", "path": "/spec/template/spec/containers/0/args/-", "value": "--kubelet-insecure-tls"}]' 2>/dev/null || true

echo ""
echo "=== Deploying plugin-core ==="
kubectl apply -f "${SCRIPT_DIR}/06-core-deployment.yaml"
kubectl apply -f "${SCRIPT_DIR}/07-core-service.yaml"

echo ""
echo "=== Waiting for core to be ready ==="
kubectl wait --for=condition=ready pod -l app=plugin-core -n plugin-core --timeout=120s

echo ""
echo "=== Initializing admin scopes ==="
kubectl apply -f "${SCRIPT_DIR}/10-init-scopes.yaml" 2>/dev/null || true

echo ""
echo "=== Status ==="
kubectl get pods -n plugin-core
kubectl get svc -n plugin-core

# Restore manifest files
cd "${SCRIPT_DIR}"
git checkout -- 02-configmap.yaml 06-core-deployment.yaml 2>/dev/null || true

echo ""
echo "=== Core is ready at http://localhost:8080 ==="
echo "Run: kubectl port-forward -n plugin-core svc/plugin-core 8080:8080 --address 0.0.0.0"
