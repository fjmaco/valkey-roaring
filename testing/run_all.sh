#!/usr/bin/env bash
# End-to-end validation runner. Starts the module server from the repo's
# docker compose, prepares the Python environment, then executes every
# suite in order. Any suite failure fails the run.
#
# Usage:  bash run_all.sh              # standard run (~3 minutes + downloads)
#         FULL=1 bash run_all.sh      # adds the large datasets to suite 01
#         bash run_all.sh 03 09       # run only the given suite numbers

set -uo pipefail
cd "$(dirname "$0")"
REPO=".."

# --- environment -----------------------------------------------------------
if [ ! -x .venv/bin/python ]; then
  python3 -m venv .venv
  .venv/bin/pip install --quiet -r requirements.txt
fi

# --- server ----------------------------------------------------------------
(cd "$REPO" && docker compose up -d)
for _ in $(seq 1 60); do
  [ "$(cd "$REPO" && docker compose exec -T valkey valkey-cli PING 2>/dev/null)" = "PONG" ] && break
  sleep 1
done

# --- suites ----------------------------------------------------------------
filter=("$@")
overall=0
declare -a summary
for suite in suites/test_*.py; do
  num=$(basename "$suite" | cut -d_ -f2)
  if [ ${#filter[@]} -gt 0 ] && [[ ! " ${filter[*]} " == *" $num "* ]]; then
    continue
  fi
  echo
  echo "================ $(basename "$suite") ================"
  if .venv/bin/python "$suite"; then
    summary+=("PASS  $(basename "$suite")")
  else
    summary+=("FAIL  $(basename "$suite")")
    overall=1
  fi
done

echo
echo "======================= SUMMARY ======================="
printf '%s\n' "${summary[@]}"
exit $overall
