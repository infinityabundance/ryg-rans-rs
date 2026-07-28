#!/bin/sh
# bootstrap-docker.sh - Set up Docker root, build images, run matrix
# Usage: ./bootstrap-docker.sh [run-id]
set -eu

DOCKER_ROOT="/run/media/one/toshiba4TB/docker/ryg-rans-rs"
PROJECT_ROOT="/run/media/one/1tb_kingston1/ryg-rans-rs"
GIT_SHA=$(cd "$PROJECT_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo 'nogit')
TIMESTAMP=$(date -u +%Y%m%dT%H%M%S)
RUN_ID="${1:-${TIMESTAMP}Z-${GIT_SHA}}"

PROJECT_NAME="ryg-rans-rs-court-${RUN_ID}"
COMPOSE_FILE="${PROJECT_ROOT}/docker/compose/matrix.yml"

echo "=== ryg-rans-rs Docker Matrix ==="
echo "Run ID:    $RUN_ID"
echo "Timestamp: $TIMESTAMP"
echo "Git SHA:   $GIT_SHA"
echo "Docker:    $DOCKER_ROOT"

# ---- Cleanup trap ----
cleanup() {
    echo ""
    echo "=== Cleanup ==="
    docker compose \
        --project-name "$PROJECT_NAME" \
        -f "$COMPOSE_FILE" \
        down --volumes 2>/dev/null || true
}
trap cleanup EXIT INT TERM

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

# ---- Create per-run oracle build context ----
ORACLE_CONTEXT="${DOCKER_ROOT}/runs/${RUN_ID}/oracle-context"
echo ""
echo "=== Creating oracle build context at ${ORACLE_CONTEXT} ==="
mkdir -p "$ORACLE_CONTEXT"

if [ ! -d /tmp/ryg_rans_upstream ]; then
    echo "ERROR: /tmp/ryg_rans_upstream not found"
    exit 1
fi

cp /tmp/ryg_rans_upstream/* "$ORACLE_CONTEXT/"
echo "Upstream source files copied"

# Write the oracle Dockerfile into the context
cat > "$ORACLE_CONTEXT/Dockerfile" << 'DOCKERFILE_EOF'
# syntax=docker/dockerfile:1
FROM debian:12-slim AS oracle-gcc

RUN apt-get update && apt-get install -y --no-install-recommends \
    g++ gcc make ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY . /workspace/

RUN cd /workspace && \
    g++ -o /usr/local/bin/rans_byte_oracle main.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && \
    g++ -o /usr/local/bin/rans64_oracle main64.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && \
    g++ -o /usr/local/bin/rans_alias_oracle main_alias.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && \
    g++ -o /usr/local/bin/rans_sse41_oracle main_simd.cpp -O3 -msse4.1 -lm -lrt -D_POSIX_C_SOURCE=199309L

LABEL org.infinityabundance.project=ryg-rans-rs
LABEL org.infinityabundance.purpose=forensic-parity-court
LABEL org.infinityabundance.managed-by=ryg-rans-rs-xtask
DOCKERFILE_EOF
echo "Oracle Dockerfile written"

# Symlink for source
ln -sfn "${SOURCE_SNAPSHOT}" "${DOCKER_ROOT}/source/current"

# Create report directories (bind-mount targets)
echo ""
echo "=== Creating report directories ==="
mkdir -p \
    "${DOCKER_ROOT}/reports/${RUN_ID}/oracle" \
    "${DOCKER_ROOT}/reports/${RUN_ID}/stable" \
    "${DOCKER_ROOT}/reports/${RUN_ID}/musl" \
    "${DOCKER_ROOT}/reports/${RUN_ID}/package" \
    "${DOCKER_ROOT}/reports/${RUN_ID}/docker"

# Copy dockerfiles for non-oracle services
cp -r "$PROJECT_ROOT/docker/dockerfiles" "$DOCKER_ROOT/dockerfiles"

# ---- Export RUN_ID and DOCKER_ROOT for compose ----
export RUN_ID
export DOCKER_ROOT

# ---- Build images ----
echo ""
echo "=== Building images ==="
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    build

# ---- Run matrix jobs ----
echo ""
echo "=== Running oracle-gcc ==="
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    run --rm oracle-gcc

echo ""
echo "=== Running rust-stable-tests ==="
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    run --rm rust-stable-tests

echo ""
echo "=== Running rust-musl-build ==="
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    run --rm rust-musl-build

echo ""
echo "=== Running package-audit ==="
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    run --rm package-audit

echo ""
echo "=== Matrix complete for run ${RUN_ID} ==="
echo "Reports: ${DOCKER_ROOT}/reports/${RUN_ID}/"
