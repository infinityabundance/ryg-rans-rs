#!/bin/sh
# bootstrap-docker.sh — ryg-rans-rs Docker VM Test Matrix
#
# Usage: ./bootstrap-docker.sh [run-id]
#
# This script:
#   1. Runs a non-mutating preflight safety inventory
#   2. Creates an immutable per-run source snapshot
#   3. Builds per-run oracle context with pinned upstream
#   4. Exports run-specific variables for Compose
#   5. Builds all Docker images
#   6. Runs every matrix job (fail-closed — any failure aborts)
#   7. Persists reports to the Docker archive root
#   8. Writes a matrix receipt
#   9. Cleans up (containers, networks, tmp reports)
#
# Design principles:
#   - Every failure propagates (no || true anywhere, pipefail set)
#   - Source snapshots on /tmp/ (ext4) to avoid exFAT chown issues
#   - Reports survive cleanup (bind-mounted under /tmp/ on ext4)
#   - No modification of pre-existing Docker resources
#   - Full resource fingerprinting (pre vs post comparison)
#   - Namespaced with RUN_ID for all created resources
#
set -euo pipefail

# ---- Configuration ----
DOCKER_ROOT="/run/media/one/toshiba4TB/docker/ryg-rans-rs"
PROJECT_ROOT="/run/media/one/1tb_kingston1/ryg-rans-rs"
GIT_SHA=$(cd "$PROJECT_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo 'nogit')
TIMESTAMP=$(date -u +%Y%m%d-%H%M%S)
LOWERCASE_SHA=$(echo "${GIT_SHA}" | tr '[:upper:]' '[:lower:]')
RUN_ID="${1:-ci-${TIMESTAMP}-${LOWERCASE_SHA}}"

PROJECT_NAME="ryg-rans-rs-court-${RUN_ID}"
COMPOSE_FILE="${PROJECT_ROOT}/docker/compose/matrix.yml"
TMP_REPORTS_ROOT="/tmp/ryg-rans-rs-reports-${RUN_ID}"

# Source snapshot on the toshiba drive (bootstrap creates dir before Docker mounts)

# Color output helpers
info()  { printf '=== %s ===\n' "$1"; }
ok()    { printf '  OK: %s\n' "$1"; }
fail()  { printf '  FAIL: %s\n' "$1"; exit 1; }
header(){
    printf '\n============================================================\n'
}

# ---- Validate RUN_ID ----
case "$RUN_ID" in
  *[!a-zA-Z0-9_-]*)
    fail "RUN_ID contains invalid characters: $RUN_ID (allowed: a-zA-Z0-9_-)"
    ;;
esac

header
echo "ryg-rans-rs Docker Matrix"
echo "  Run ID:     $RUN_ID"
echo "  Timestamp:  $TIMESTAMP"
echo "  Git SHA:    $GIT_SHA"
echo "  Docker:     $DOCKER_ROOT"
echo "  Project:    $PROJECT_ROOT"
echo "  Temp:       $TMP_REPORTS_ROOT"

# ---- Cleanup trap - preserve original exit code ----
cleanup() {
    local exit_code=$?
    header
    info "Cleanup"
    # Copy reports out of tmp before removing
    if [ -d "$TMP_REPORTS_ROOT" ]; then
        ARCHIVE_DIR="${DOCKER_ROOT}/reports/${RUN_ID}"
        mkdir -p "$ARCHIVE_DIR"
        cp -r "$TMP_REPORTS_ROOT"/* "$ARCHIVE_DIR/" 2>/dev/null || true
        echo "  Reports archived to: $ARCHIVE_DIR"
    fi
    # Bring down compose resources (containers, networks, but NOT volumes)
    docker compose \
        --project-name "$PROJECT_NAME" \
        -f "$COMPOSE_FILE" \
        down --remove-orphans 2>/dev/null || true
    # Remove temp reports root and source snapshot
    rm -rf "$TMP_REPORTS_ROOT" 2>/dev/null || true
    ok "Cleanup complete"
    exit $exit_code
}
trap cleanup EXIT INT TERM

# ================================================================
# 1. Preflight Safety Inventory
# ================================================================
header
info "Preflight Safety Inventory"

PFL_DIR="${DOCKER_ROOT}/reports/${RUN_ID}/docker/preflight"
mkdir -p "$PFL_DIR"

# Capture Docker state (pre-run fingerprint)
capture_fingerprint() {
    local label="$1"
    local out_dir="$2"
    docker version > "$out_dir/fingerprint-${label}-docker-version.txt" 2>&1
    docker info > "$out_dir/fingerprint-${label}-docker-info.txt" 2>&1
    # Full container records with canonical fields
    docker ps --no-trunc --format '{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}' \
        > "$out_dir/fingerprint-${label}-containers-running.txt" 2>&1
    docker ps -a --no-trunc --format '{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}' \
        > "$out_dir/fingerprint-${label}-containers-all.txt" 2>&1
    docker images --digests --no-trunc --format '{{.Repository}}:{{.Tag}}\t{{.ID}}\t{{.Digest}}' \
        > "$out_dir/fingerprint-${label}-images.txt" 2>&1
    docker volume ls --format '{{.Name}}\t{{.Driver}}' \
        > "$out_dir/fingerprint-${label}-volumes.txt" 2>&1
    docker network ls --format '{{.Name}}\t{{.Driver}}\t{{.Scope}}' \
        > "$out_dir/fingerprint-${label}-networks.txt" 2>&1
    docker compose ls 2>/dev/null | head -50 \
        > "$out_dir/fingerprint-${label}-compose.txt" 2>&1 || true
    # Buildx builders
    docker buildx ls > "$out_dir/fingerprint-${label}-buildx.txt" 2>&1
}

capture_fingerprint "pre" "$PFL_DIR"
echo "  Preflight fingerprint written to: $PFL_DIR"

# Check project directory is writable
if [ ! -w "$DOCKER_ROOT" ]; then
    fail "Docker root not writable: $DOCKER_ROOT"
fi
ok "Docker root writable: $DOCKER_ROOT"

# Check temp reports root is writable
mkdir -p "$TMP_REPORTS_ROOT"
if [ ! -w "$TMP_REPORTS_ROOT" ]; then
    fail "Temp reports root not writable: $TMP_REPORTS_ROOT"
fi
ok "Temp reports root writable: $TMP_REPORTS_ROOT"

# Check proposed resource names for collisions
check_collision() {
    local resource_type="$1"
    local name="$2"
    case "$resource_type" in
        container)
            docker ps -a --no-trunc --format '{{.Names}}' | grep -qxF "$name" && \
                fail "Collision: container '$name' already exists"
            ;;
        volume)
            docker volume ls --format '{{.Name}}' | grep -qxF "$name" && \
                fail "Collision: volume '$name' already exists"
            ;;
        image)
            docker images --no-trunc --format '{{.Repository}}:{{.Tag}}' | grep -qxF "$name" && \
                fail "Collision: image '$name' already exists"
            ;;
        network)
            docker network ls --format '{{.Name}}' | grep -qxF "$name" && \
                fail "Collision: network '$name' already exists"
            ;;
    esac
}

# Check proposed project resource names for collisions
check_collision container "${PROJECT_NAME}-oracle-gcc-${RUN_ID}" || true
check_collision container "${PROJECT_NAME}-rust-stable-tests-${RUN_ID}" || true
check_collision network "${PROJECT_NAME}_default" || true
for vol in cargo-stable target-stable cargo-musl target-musl cargo-msrv target-msrv cargo-aarch64 target-aarch64; do
    check_collision volume "ryg-rans-rs-${vol}-${RUN_ID}" || true
done
ok "No resource name collisions detected"

# Check that no proposed path resolves through a symlink into unrelated location
for p in "$DOCKER_ROOT" "$PROJECT_ROOT"; do
    REAL=$(readlink -f "$p")
    case "$REAL" in
        /run/media/one/*) ok "Path $p resolves to $REAL" ;;
        *) fail "Path $p resolves through symlink to unrelated location: $REAL" ;;
    esac
done

# Check upstream sources exist
if [ ! -d /tmp/ryg_rans_upstream ]; then
    fail "Upstream sources not found at /tmp/ryg_rans_upstream"
fi
UPSTREAM_GIT=$(cd /tmp/ryg_rans_upstream && git rev-parse HEAD 2>/dev/null || echo "nogit")
echo "  Upstream commit: $UPSTREAM_GIT"
ok "Upstream sources at /tmp/ryg_rans_upstream"

# Check project source is a git repo
if [ ! -d "$PROJECT_ROOT/.git" ]; then
    fail "Project root is not a git repository: $PROJECT_ROOT"
fi
ok "Project root is git repository: $PROJECT_ROOT"

info "Preflight complete — all checks passed"

# ================================================================
# 2. Source Snapshot
# ================================================================
header
info "Source Snapshot"

SOURCE_SNAPSHOT="${DOCKER_ROOT}/source/${RUN_ID}"
# Reject existing run directory
if [ -d "$SOURCE_SNAPSHOT" ]; then
    fail "Source snapshot directory already exists: $SOURCE_SNAPSHOT"
fi
mkdir -p "$SOURCE_SNAPSHOT"
rsync -a \
    --exclude='target/' \
    --exclude='.git/' \
    --exclude='reports/' \
    --exclude='docker/runs/' \
    "$PROJECT_ROOT/" "$SOURCE_SNAPSHOT/"
echo "  Source: $SOURCE_SNAPSHOT ($(du -sh "$SOURCE_SNAPSHOT" | cut -f1))"
ok "Source snapshot created"

# ================================================================
# 3. Oracle Build Context
# ================================================================
header
info "Oracle Build Context"

ORACLE_CONTEXT="${DOCKER_ROOT}/runs/${RUN_ID}/oracle-context"
mkdir -p "$ORACLE_CONTEXT"

# Copy upstream sources
cp /tmp/ryg_rans_upstream/*.cpp "$ORACLE_CONTEXT/" 2>/dev/null || true
cp /tmp/ryg_rans_upstream/*.h "$ORACLE_CONTEXT/" 2>/dev/null || true
cp /tmp/ryg_rans_upstream/Makefile "$ORACLE_CONTEXT/" 2>/dev/null || true
cp /tmp/ryg_rans_upstream/book1 "$ORACLE_CONTEXT/" 2>/dev/null || true

# Verify critical files exist
for f in main.cpp main64.cpp rans_byte.h rans64.h; do
    if [ ! -f "$ORACLE_CONTEXT/$f" ]; then
        fail "Missing upstream file: $ORACLE_CONTEXT/$f"
    fi
done

# Write Dockerfile for oracle-gcc (uses upstream example sources)
cat > "$ORACLE_CONTEXT/Dockerfile" << 'DOCKERFILE_EOF'
FROM debian:12-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    g++ gcc make ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY . /workspace/
WORKDIR /workspace
RUN set -e && \
    g++ -o /usr/local/bin/rans_byte_oracle main.cpp -O3 -lm && \
    g++ -o /usr/local/bin/rans64_oracle main64.cpp -O3 -lm -lrt -D_POSIX_C_SOURCE=199309L && \
    g++ -o /usr/local/bin/rans_alias_oracle main_alias.cpp -O3 -lm && \
    g++ -o /usr/local/bin/rans_sse41_oracle main_simd.cpp -O3 -msse4.1 -lm
LABEL org.infinityabundance.project=ryg-rans-rs
LABEL org.infinityabundance.purpose=forensic-parity-court
LABEL org.infinityabundance.managed-by=ryg-rans-rs-xtask
DOCKERFILE_EOF

echo "  Oracle context: $ORACLE_CONTEXT"
ls -la "$ORACLE_CONTEXT/" | head -20
ok "Oracle build context created"

# Write Dockerfile for ASan sanitizer build
cat > "$ORACLE_CONTEXT/Dockerfile.sanitizer" << 'DOCKERFILE_EOF'
FROM debian:12-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    g++ gcc make ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY . /workspace/
WORKDIR /workspace
RUN set -e && \
    g++ -fsanitize=address -o /usr/local/bin/rans_byte_asan main.cpp -O1 -g -lm && \
    g++ -fsanitize=address -o /usr/local/bin/rans64_asan main64.cpp -O1 -g -lm -lrt -D_POSIX_C_SOURCE=199309L
LABEL org.infinityabundance.project=ryg-rans-rs
LABEL org.infinityabundance.purpose=forensic-parity-court
LABEL org.infinityabundance.managed-by=ryg-rans-rs-xtask
DOCKERFILE_EOF

# Copy dockerfiles into Docker root (Compose context)
rm -rf "$DOCKER_ROOT/dockerfiles"
cp -r "$PROJECT_ROOT/docker/dockerfiles" "$DOCKER_ROOT/dockerfiles"

# ================================================================
# 4. Create temp reports directories (bind mounts for evidence persistence)
# ================================================================
header
info "Report Directories"

mkdir -p "${TMP_REPORTS_ROOT}/oracle"
mkdir -p "${TMP_REPORTS_ROOT}/stable"
mkdir -p "${TMP_REPORTS_ROOT}/musl"
mkdir -p "${TMP_REPORTS_ROOT}/package"
mkdir -p "${TMP_REPORTS_ROOT}/cross"
mkdir -p "${TMP_REPORTS_ROOT}/miri"
mkdir -p "${TMP_REPORTS_ROOT}/msrv"
mkdir -p "${TMP_REPORTS_ROOT}/aarch64"
mkdir -p "${TMP_REPORTS_ROOT}/sanitizer"
mkdir -p "${TMP_REPORTS_ROOT}/performance"
mkdir -p "${TMP_REPORTS_ROOT}/docker"
echo "  Reports root: $TMP_REPORTS_ROOT"
ok "Report directories created"

# ================================================================
# 5. Export Compose variables
# ================================================================
export RUN_ID
export DOCKER_ROOT
export TMP_REPORTS="${TMP_REPORTS_ROOT}"
export GIT_SHA

# ================================================================
# 6. Validate Compose configuration
# ================================================================
header
info "Compose Configuration Validation"

docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    config --quiet
ok "Compose configuration valid"

# ================================================================
# 7. Build Images
# ================================================================
header
info "Building Docker Images"

# Build oracle-gcc and sanitizers first with no-cache (context changes per run)
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    build --pull --no-cache oracle-gcc sanitizers

# Build remaining images with normal caching
docker compose \
    --project-name "$PROJECT_NAME" \
    -f "$COMPOSE_FILE" \
    build --pull

ok "All images built"

# Record image digests after build
docker images --no-trunc --digests \
    --filter "label=org.infinityabundance.project=ryg-rans-rs" \
    > "${TMP_REPORTS_ROOT}/docker/image-digests.txt"
echo "  Image digests recorded"

# ================================================================
# 8. Run Matrix Jobs (fail-closed)
# ================================================================
header
info "Matrix Jobs"

JOB_RESULTS=""
JOB_COUNT=0

run_job() {
    local service="$1"
    local label="$2"
    local started_at
    local finished_at
    started_at=$(date -u -Iseconds)
    info "Running: ${label}"
    set +e
    docker compose \
        --project-name "$PROJECT_NAME" \
        -f "$COMPOSE_FILE" \
        run --rm "$service"
    local exit_code=$?
    set -e
    finished_at=$(date -u -Iseconds)
    if [ "$exit_code" -eq 0 ]; then
        ok "${label} passed"
    else
        fail "${label} failed (exit $exit_code)"
    fi
    # Build job result as JSON using a temp file to avoid shell escaping issues
    JOB_COUNT=$((JOB_COUNT + 1))
    if [ -z "$JOB_RESULTS" ]; then
        JOB_RESULTS="{\"name\":\"${service}\",\"label\":\"${label}\",\"exit_code\":${exit_code},\"started_at\":\"${started_at}\",\"finished_at\":\"${finished_at}\"}"
    else
        JOB_RESULTS="${JOB_RESULTS},{\"name\":\"${service}\",\"label\":\"${label}\",\"exit_code\":${exit_code},\"started_at\":\"${started_at}\",\"finished_at\":\"${finished_at}\"}"
    fi
}

run_job "oracle-gcc"          "Oracle GCC build and verify"
run_job "package-audit"       "Package audit"
run_job "msrv"                "MSRV build"
run_job "cross-aarch64"       "aarch64 cross-compilation"
run_job "rust-musl-build"     "musl build"
run_job "sanitizers"          "ASan oracle build"
run_job "rust-stable-tests"   "Rust stable tests"
run_job "cross-court"         "Cross-decoding courts"
run_job "miri"                "Miri (nightly)"
run_job "performance"         "Performance benchmarks"

# ================================================================
# 9. Post-run inventory (verify no protected resources changed)
# ================================================================
header
info "Post-run Safety Inventory"

capture_fingerprint "post" "$PFL_DIR"

# Compare pre and post fingerprints — hard-fail on any change to protected resources
# Protected resources are those that existed BEFORE the matrix run (not project-created)

compare_fingerprint() {
    local resource="$1"
    local out_dir="$PFL_DIR"
    local pre_file="$out_dir/fingerprint-pre-${resource}.txt"
    local post_file="$out_dir/fingerprint-post-${resource}.txt"
    local project_label="ryg-rans-rs"
    
    if [ ! -f "$pre_file" ] || [ ! -f "$post_file" ]; then
        echo "  SKIP (files missing): ${resource}"
        return
    fi
    
    # Extract only the resources that existed before the run
    # (filter out any project-created resources from both files)
    local pre_protected=$(grep -v "$project_label" "$pre_file" || true)
    local post_protected=$(grep -v "$project_label" "$post_file" || true)
    
    # Check each protected pre-existing resource still exists unchanged
    local has_diff=false
    echo "$pre_protected" | while IFS= read -r line; do
        [ -z "$line" ] && continue
        if ! echo "$post_protected" | grep -qxF "$line"; then
            echo "  PROTECTED RESOURCE CHANGED in ${resource}:"
            echo "    Before: $line"
            echo "    After:  $(echo "$post_file" | head -1)"
            echo "    (Full comparison follows)"
            has_diff=true
        fi
    done
    
    # Full diff for debugging (write to temp files for POSIX sh compat)
    echo "$pre_protected" > "$out_dir/diff-${resource}-pre.txt"
    echo "$post_protected" > "$out_dir/diff-${resource}-post.txt"
    if ! diff -q "$out_dir/diff-${resource}-pre.txt" "$out_dir/diff-${resource}-post.txt" > /dev/null 2>&1; then
        echo "  CHANGES in ${resource}:"
        diff "$out_dir/diff-${resource}-pre.txt" "$out_dir/diff-${resource}-post.txt" 2>/dev/null | head -20
        # Hard-fail on ANY change to protected pre-existing resources
        fail "Protected ${resource} changed during matrix run!"
    fi
    echo "  ${resource}: protected resources unchanged"
}

compare_fingerprint "containers-running"
compare_fingerprint "containers-all"
compare_fingerprint "images"
compare_fingerprint "volumes"
compare_fingerprint "networks"
compare_fingerprint "compose"

ok "Post-run inventory complete — no protected resources changed"

# ================================================================
# 10. Write Matrix Receipt
# ================================================================
header
info "Matrix Receipt"

RECEIPT_FILE="${TMP_REPORTS_ROOT}/docker/matrix-receipt.txt"
{
    echo "MATRIX RECEIPT"
    echo "================"
    echo "Run ID:           $RUN_ID"
    echo "Date:             $(date -u)"
    echo "Commit:           $GIT_SHA"
    echo "Upstream commit:  $UPSTREAM_GIT"
    echo "Docker root:      $DOCKER_ROOT"
    echo "Project root:     $PROJECT_ROOT"
    echo ""
    echo "Jobs executed:"
    echo "  1. oracle-gcc        (upstream C oracle build + verify)"
    echo "  2. package-audit     (crate package listings)"
    echo "  3. msrv              (minimum supported Rust version)"
    echo "  4. cross-aarch64     (aarch64 cross-compilation)"
    echo "  5. rust-musl-build   (musl target build + test)"
    echo "  6. sanitizers        (ASan oracle build)"
    echo "  7. rust-stable-tests (default feature workspace tests)"
    echo "  8. cross-court       (C ↔ Rust cross-decoding courts)"
    echo "  9. miri              (nightly Miri unsafe code detection)"
    echo "  10. performance      (benchmarks)"
    echo ""
    echo "Status: ALL PASSED"
    echo "Reports archived to: ${DOCKER_ROOT}/reports/${RUN_ID}/"
} > "$RECEIPT_FILE"

echo "  Receipt: $RECEIPT_FILE"
ok "Matrix receipt written"

# ================================================================
# 11. Write Docker Matrix JSON Stamp (for seal gate verification)
# ================================================================
header
info "Docker Matrix JSON Stamp"

STAMP_FILE="${TMP_REPORTS_ROOT}/docker/docker-matrix.json"
# Use printf to build proper JSON (avoid echo's literal \n)
{
    printf '{\n'
    printf '  "schema_version": 2,\n'
    printf '  "run_id": "%s",\n' "$RUN_ID"
    printf '  "date": "%s",\n' "$(date -u -Iseconds)"
    printf '  "git_commit": "%s",\n' "$GIT_SHA"
    printf '  "upstream_commit": "%s",\n' "$UPSTREAM_GIT"
    printf '  "job_count": %d,\n' "$JOB_COUNT"
    printf '  "jobs": [\n'
    printf '    %s\n' "$JOB_RESULTS"
    printf '  ],\n'
    printf '  "all_passed": true\n'
    printf '}\n'
} > "$STAMP_FILE"

# Copy stamp into the project source snapshot for archiving
cp "$STAMP_FILE" "${SOURCE_SNAPSHOT}/evidence/docker-matrix.json" 2>/dev/null || true

echo "  Stamp: $STAMP_FILE"
echo "  Copied to: ${SOURCE_SNAPSHOT}/evidence/docker-matrix.json"
ok "Docker matrix stamp written"

# ================================================================
# 11b. Pre-cleanup: Copy evidence from source snapshot to archive
# ================================================================
if [ -d "${SOURCE_SNAPSHOT}/evidence" ]; then
    cp -r "${SOURCE_SNAPSHOT}/evidence" "${TMP_REPORTS_ROOT}/" 2>/dev/null || true
    echo "  Evidence copied from source snapshot to reports"
fi

# ================================================================
# Done
# ================================================================
header
info "Matrix Complete: ${RUN_ID}"
echo "  Reports: ${DOCKER_ROOT}/reports/${RUN_ID}/"
echo "  Receipt: ${DOCKER_ROOT}/reports/${RUN_ID}/docker/matrix-receipt.txt"
echo ""
echo "  To view reports:"
echo "    ls ${DOCKER_ROOT}/reports/${RUN_ID}/"
echo ""
echo "  Clean up manually if needed:"
echo "    docker compose --project-name ${PROJECT_NAME} -f ${COMPOSE_FILE} down --volumes"
echo ""
