#!/bin/sh
# bootstrap-docker.sh - Set up Docker root, build images, run matrix
set -eu

DOCKER_ROOT="/run/media/one/toshiba4TB/docker/ryg-rans-rs"
PROJECT_ROOT="/run/media/one/1tb_kingston1/ryg-rans-rs"
RUN_ID="$(date -u +%Y%m%dT%H%M%S)-$(cd "$PROJECT_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo 'nogit')"

echo "=== Bootstrap Docker Matrix ==="
echo "Run ID: $RUN_ID"
echo "Docker root: $DOCKER_ROOT"

# Setup target directories
mkdir -p "$DOCKER_ROOT/target/stable" "$DOCKER_ROOT/target/musl" \
         "$DOCKER_ROOT/reports/stable" "$DOCKER_ROOT/reports/musl" \
         "$DOCKER_ROOT/reports/package" "$DOCKER_ROOT/source"

# Copy upstream source
if [ -d /tmp/ryg_rans_upstream ]; then
    mkdir -p "$DOCKER_ROOT/upstream"
    cp -r /tmp/ryg_rans_upstream/* "$DOCKER_ROOT/upstream/"
    echo "Upstream source copied"
else
    echo "ERROR: /tmp/ryg_rans_upstream not found"
    exit 1
fi

# Copy dockerfiles from project to docker root
cp -r "$PROJECT_ROOT/docker/dockerfiles" "$DOCKER_ROOT/dockerfiles"
cp -r "$PROJECT_ROOT/docker/compose" "$DOCKER_ROOT/compose"

# Build images
echo "=== Building oracle-gcc ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    build oracle-gcc

echo "=== Building rust-stable ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    build rust-stable

echo "=== Building rust-musl ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    build rust-musl

echo "=== Building package-audit ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    build package-audit

# Run matrix jobs
echo "=== Running oracle-gcc ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    run --rm oracle-gcc

echo "=== Running rust-stable tests ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    run --rm rust-stable

echo "=== Running rust-musl build ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    run --rm rust-musl

echo "=== Running package-audit ==="
docker compose \
    --project-name "ryg-rans-rs-court-${RUN_ID}" \
    -f "$DOCKER_ROOT/compose/matrix.yml" \
    run --rm package-audit

echo "=== Matrix complete for run $RUN_ID ==="
