#!/usr/bin/env bash
set -euo pipefail

DEFAULT_VERSION="0.1.0"
REGISTRY="localhost:5000"

read -rp "Version (default: ${DEFAULT_VERSION}): " VERSION
VERSION="${VERSION:-$DEFAULT_VERSION}"

IMAGE="${REGISTRY}/alcedocore/sp-admin-ui:${VERSION}"

echo "Building ${IMAGE}..."
docker build -f "$(dirname "$0")/Dockerfile" -t "${IMAGE}" "$(dirname "$0")/../../"

echo "Pushing ${IMAGE}..."
docker push "${IMAGE}"

echo "Done: ${IMAGE}"
