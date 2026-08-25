#!/usr/bin/env bash
# Performance benchmark for valkey-roaring, replicating redis-roaring's
# performance.sh: runs the benchmark harness against the dockerized Valkey
# and splices the resulting table into README.md between the
# BEGIN_PERFORMANCE / END_PERFORMANCE markers.
#
# Usage: bash tests/performance.sh
#   PERF_MAX_FILES=N   limit dataset files for a quick smoke run
#                      (README is only updated on full runs)

set -euo pipefail

cd "$(dirname "$0")/.."

DATASET_DIR="tests/performance/data/census1881"
DATASET_URL="https://github.com/RoaringBitmap/real-roaring-datasets/raw/master/census1881.zip"
CLI="docker compose exec -T valkey valkey-cli"

# --- Dataset (CRoaring census1881, same as upstream) ---
if [ ! -d "$DATASET_DIR" ] || [ -z "$(ls "$DATASET_DIR"/*.txt 2>/dev/null)" ]; then
  echo "Downloading census1881 dataset..."
  mkdir -p "$(dirname "$DATASET_DIR")"
  curl -sL -o "$DATASET_DIR.zip" "$DATASET_URL"
  unzip -q -o "$DATASET_DIR.zip" -d "$DATASET_DIR"
  rm -f "$DATASET_DIR.zip"
fi

# --- Build the harness ---
cargo build --release --quiet --manifest-path tests/performance/Cargo.toml

# --- Server ---
docker compose up -d
for _ in $(seq 1 30); do
  if [ "$($CLI PING 2>/dev/null)" = "PONG" ]; then break; fi
  sleep 1
done
$CLI FLUSHALL > /dev/null

# --- Run ---
PERF_OUTPUT_FILE=$(mktemp)
tests/performance/target/release/performance "$DATASET_DIR" | tee "$PERF_OUTPUT_FILE"

$CLI FLUSHALL > /dev/null

PERF_TABLE=$(grep -E '^\|' "$PERF_OUTPUT_FILE")
rm -f "$PERF_OUTPUT_FILE"

# --- Update README.md (full runs only) ---
if [ -n "${PERF_MAX_FILES:-}" ]; then
  echo "PERF_MAX_FILES set — skipping README.md update"
  exit 0
fi

if [ -f "README.md" ] && [ -n "$PERF_TABLE" ]; then
  echo "Updating README.md with latest performance results..."
  awk -v perf="$PERF_TABLE" '
    /<!-- BEGIN_PERFORMANCE -->/ {
      print
      print perf
      skip=1
      next
    }
    /<!-- END_PERFORMANCE -->/ {
      skip=0
    }
    !skip
  ' README.md > README.md.tmp
  mv README.md.tmp README.md
  echo "README.md updated successfully"
fi
