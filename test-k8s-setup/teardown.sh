#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME="plugin-core-test"

echo "=== Deleting K3d cluster: $CLUSTER_NAME ==="
k3d cluster delete "$CLUSTER_NAME" 2>/dev/null || true

echo ""
echo "=== Cleanup complete ==="
