#!/bin/bash
# Run e2e tests against a K8s cluster.
# Creates a docker -> kubectl wrapper so `docker compose exec` calls
# in the test suite are redirected to kubectl exec.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Install the docker -> kubectl wrapper
cat > /usr/local/bin/docker << 'DOCKERWRAPPER'
#!/bin/bash
if [[ "$1" == "compose" && "$2" == "exec" ]]; then
    shift 2
    if [[ "$1" == "-T" ]]; then shift 1; fi
    CONTAINER="$1"; shift 1
    case "$CONTAINER" in
        postgres) exec kubectl exec -i -n plugin-core postgres-0 -- "$@" ;;
        redis) exec kubectl exec -i -n plugin-core deploy/redis -- "$@" ;;
        *) echo "Unknown container: $CONTAINER" >&2; exit 1 ;;
    esac
fi
exec /usr/bin/docker "$@"
DOCKERWRAPPER
chmod +x /usr/local/bin/docker

echo "[K8s-tests] docker wrapper installed"

# Run the tests
cd "$SCRIPT_DIR/../e2e-tests"
npm run test:api

# Clean up
rm -f /usr/local/bin/docker
echo "[K8s-tests] docker wrapper removed"
