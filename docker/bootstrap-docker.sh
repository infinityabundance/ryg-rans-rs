#!/bin/sh
# bootstrap-docker.sh - Set up Docker root, build images, run matrix
# Usage: ./bootstrap-docker.sh [run-id]
set -eu

DOCKER_ROOT="/run/media/one/toshiba4TB/docker/ryg-rans-rs"
PROJECT_ROOT="/run/media/one/1tb_kingston1/ryg-rans-rs"
GIT_SHA=$(cd "$PROJECT_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo 'nogit')
TIMESTAMP=$(date -u +%Y%m%dT%H%M%S)
RUN_ID="${1:-ci-${TIMESTAMP}-${GIT_SHA}}"

PROJECT_NAME="ryg-rans-rs-court-${RUN_ID}"
COMPOSE_FILE="${PROJECT_ROOT}/docker/compose/matrix.yml"

echo "=== ryg-rans-rs Docker Matrix ==="
echo "Run ID:    $RUN_ID"
echo "Timestamp: $TIMESTAMP"
echo "Git SHA:   $GIT_SHA"
echo "Docker:    $DOCKER_ROOT"

# ---- Cleanup trap ----
cleanup() {
    echo "" && echo "=== Cleanup ==="
    docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" down --volumes 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ---- Preflight ----
echo "" && echo "=== Preflight Check ==="
EXISTING=$(docker ps -a --no-trunc --filter "label=org.infinityabundance.project=ryg-rans-rs" -q | wc -l)
if [ "$EXISTING" -gt 0 ]; then
    echo "ERROR: $EXISTING existing project containers found"
    exit 1
fi
if [ ! -w "$DOCKER_ROOT" ]; then
    echo "ERROR: Docker root not writable: $DOCKER_ROOT"
    exit 1
fi
echo "Preflight OK"

# ---- Source snapshot ----
SOURCE_SNAPSHOT="${DOCKER_ROOT}/source/${RUN_ID}"
echo "" && echo "=== Source snapshot ==="
mkdir -p "$SOURCE_SNAPSHOT"
rsync -a --delete --exclude='target/' --exclude='.git/' --exclude='reports/' "$PROJECT_ROOT/" "$SOURCE_SNAPSHOT/"
echo "Source: $SOURCE_SNAPSHOT ($(du -sh $SOURCE_SNAPSHOT | cut -f1))"

# ---- Oracle build context ----
ORACLE_CONTEXT="${DOCKER_ROOT}/runs/${RUN_ID}/oracle-context"
echo "" && echo "=== Oracle context ==="
mkdir -p "$ORACLE_CONTEXT"
if [ ! -d /tmp/ryg_rans_upstream ]; then echo "ERROR: /tmp/ryg_rans_upstream not found"; exit 1; fi
cp /tmp/ryg_rans_upstream/* "$ORACLE_CONTEXT/"
cat > "$ORACLE_CONTEXT/Dockerfile" << 'DOCKERFILE_EOF'
FROM debian:12-slim
RUN apt-get update && apt-get install -y --no-install-recommends g++ gcc make ca-certificates && rm -rf /var/lib/apt/lists/*
COPY . /workspace/
RUN cd /workspace && g++ -o /usr/local/bin/rans_byte_oracle main.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && g++ -o /usr/local/bin/rans64_oracle main64.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && g++ -o /usr/local/bin/rans_alias_oracle main_alias.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && g++ -o /usr/local/bin/rans_sse41_oracle main_simd.cpp -O3 -msse4.1 -lm -lrt -D_POSIX_C_SOURCE=199309L
LABEL org.infinityabundance.project=ryg-rans-rs org.infinityabundance.purpose=forensic-parity-court org.infinityabundance.managed-by=ryg-rans-rs-xtask
DOCKERFILE_EOF
echo "Oracle context created"

# ---- Report directories ----
mkdir -p "${DOCKER_ROOT}/reports/${RUN_ID}/oracle" "${DOCKER_ROOT}/reports/${RUN_ID}/stable" "${DOCKER_ROOT}/reports/${RUN_ID}/musl" "${DOCKER_ROOT}/reports/${RUN_ID}/package" "${DOCKER_ROOT}/reports/${RUN_ID}/docker" "${DOCKER_ROOT}/reports/${RUN_ID}/miri"

# Copy dockerfiles
cp -r "$PROJECT_ROOT/docker/dockerfiles" "$DOCKER_ROOT/dockerfiles"

# Export for compose
export RUN_ID DOCKER_ROOT

# ---- Build ----
echo "" && echo "=== Building images ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" build

# ---- Run: oracle-gcc ----
echo "" && echo "=== Running oracle-gcc ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" run --rm oracle-gcc

# ---- Run: rust-stable-tests ----
echo "" && echo "=== Running rust-stable-tests ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" run --rm rust-stable-tests

# ---- Run: rust-musl-build ----
echo "" && echo "=== Running rust-musl-build ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" run --rm rust-musl-build

# ---- Run: package-audit ----
echo "" && echo "=== Running package-audit ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" run --rm package-audit

# ---- Run: cross-court ----
echo "" && echo "=== Running cross-court ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" run --rm cross-court

# ---- Run: miri ----
echo "" && echo "=== Running miri ==="
docker compose --project-name "$PROJECT_NAME" -f "$COMPOSE_FILE" run --rm miri

# ---- Matrix receipt ----
echo "" && echo "=== Writing matrix receipt ==="
RECEIPT_FILE="${DOCKER_ROOT}/reports/${RUN_ID}/docker/matrix-receipt.txt"
{
    echo "MATRIX RECEIPT"
    echo "Run ID: $RUN_ID"
    echo "Date: $(date -u)"
    echo "Commit: $GIT_SHA"
    echo "Jobs: oracle, stable-tests, musl, package, cross-court, miri"
} > "$RECEIPT_FILE"
echo "Receipt: $RECEIPT_FILE"

echo "" && echo "=== Matrix complete: ${RUN_ID} ==="
echo "Reports: ${DOCKER_ROOT}/reports/${RUN_ID}/"
