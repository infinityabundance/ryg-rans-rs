#!/bin/sh
# bootstrap-docker.sh - Set up Docker root, build images, run matrix
# Usage: ./bootstrap-docker.sh [run-id]
set -eu

DOCKER_ROOT="/run/media/one/toshiba4TB/docker/ryg-rans-rs"
PROJECT_ROOT="/run/media/one/1tb_kingston1/ryg-rans-rs"
GIT_SHA=$(cd "$PROJECT_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo 'nogit')
TIMESTAMP=$(date -u +%Y%m%dT%H%M%S)
RUN_ID="${1:-${TIMESTAMP}Z-${GIT_SHA}}"

echo "=== ryg-rans-rs Docker Matrix ==="
echo "Run ID:    $RUN_ID"
echo "Timestamp: $TIMESTAMP"
echo "Git SHA:   $GIT_SHA"
echo "Docker:    $DOCKER_ROOT"

# ---- Preflight: check no existing project resources ----
echo ""
echo "=== Preflight Check ==="
EXISTING_CONTAINERS=$(docker ps -a --no-trunc --filter "label=org.infinityabundance.project=ryg-rans-rs" -q | wc -l)
if [ "$EXISTING_CONTAINERS" -gt 0 ]; then
    echo "ERROR: Found $EXISTING_CONTAINERS existing project containers with matching labels."
    echo "Run cleanup first: docker rm \$(docker ps -a -q --filter 'label=org.infinityabundance.project=ryg-rans-rs')"
    exit 1
fi

# Check Docker root is writable
if [ ! -w "$DOCKER_ROOT" ]; then
    echo "ERROR: Docker root not writable: $DOCKER_ROOT"
    exit 1
fi

# ---- Create source snapshot ----
SOURCE_SNAPSHOT="${DOCKER_ROOT}/source/${RUN_ID}"
echo ""
echo "=== Creating source snapshot at ${SOURCE_SNAPSHOT} ==="
mkdir -p "$SOURCE_SNAPSHOT"
rsync -a --delete \
    --exclude='target/' \
    --exclude='.git/' \
    --exclude='reports/' \
    "$PROJECT_ROOT/" "$SOURCE_SNAPSHOT/"
echo "Source snapshot created ($(du -sh $SOURCE_SNAPSHOT | cut -f1))"

# Copy upstream source
mkdir -p "${DOCKER_ROOT}/upstream/${RUN_ID}"
if [ -d /tmp/ryg_rans_upstream ]; then
    cp -r /tmp/ryg_rans_upstream/* "${DOCKER_ROOT}/upstream/${RUN_ID}/"
    echo "Upstream source copied"
else
    echo "ERROR: /tmp/ryg_rans_upstream not found"
    exit 1
fi

# Symlink for build context
ln -sfn "${DOCKER_ROOT}/upstream/${RUN_ID}" "${DOCKER_ROOT}/upstream/current"
ln -sfn "${SOURCE_SNAPSHOT}" "${DOCKER_ROOT}/source/current"

# Create target directories
mkdir -p "${DOCKER_ROOT}/target/stable" "${DOCKER_ROOT}/target/musl" \
         "${DOCKER_ROOT}/reports/${RUN_ID}/stable" \
         "${DOCKER_ROOT}/reports/${RUN_ID}/musl" \
         "${DOCKER_ROOT}/reports/${RUN_ID}/package" \
         "${DOCKER_ROOT}/reports/${RUN_ID}/docker"

# Copy dockerfiles
cp -r "$PROJECT_ROOT/docker/dockerfiles" "$DOCKER_ROOT/dockerfiles"

# ---- Export RUN_ID for compose ----
export RUN_ID
export DOCKER_ROOT

# ---- Build images ----
echo ""
echo "=== Building images ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$PROJECT_ROOT/docker/compose/matrix.yml" \
    build 2>&1 | grep -E "(Built|error|ERROR)" || true

# ---- Run matrix jobs ----
echo ""
echo "=== Running oracle-gcc ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$PROJECT_ROOT/docker/compose/matrix.yml" \
    run --rm oracle-gcc 2>&1 || echo "WARNING: oracle-gcc failed"

echo ""
echo "=== Running rust-stable-tests ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$PROJECT_ROOT/docker/compose/matrix.yml" \
    run --rm rust-stable-tests 2>&1 || echo "WARNING: rust-stable-tests failed"

echo ""
echo "=== Running rust-musl-build ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$PROJECT_ROOT/docker/compose/matrix.yml" \
    run --rm rust-musl-build 2>&1 || echo "WARNING: rust-musl-build failed"

echo ""
echo "=== Running package-audit ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$PROJECT_ROOT/docker/compose/matrix.yml" \
    run --rm package-audit 2>&1 || echo "WARNING: package-audit failed"

# ---- Cleanup ----
echo ""
echo "=== Cleanup ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$PROJECT_ROOT/docker/compose/matrix.yml" \
    down --volumes 2>&1

echo ""
echo "=== Matrix complete for run ${RUN_ID} ==="
echo "Reports: ${DOCKER_ROOT}/reports/${RUN_ID}/"
