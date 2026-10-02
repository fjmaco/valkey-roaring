#!/usr/bin/env bash
# Integration test suite for valkey-roaring module.
# Runs against a live Valkey instance via docker compose.

set -euo pipefail

CLI="docker compose exec -T valkey valkey-cli"
PASS=0
FAIL=0
ERRORS=""

assert_eq() {
  local test_name="$1"
  local expected="$2"
  local actual="$3"
  if [[ "$actual" == "$expected" ]]; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    ERRORS="${ERRORS}\n  FAIL: ${test_name}\n    expected: '${expected}'\n    actual:   '${actual}'"
    echo "  FAIL: ${test_name}"
  fi
}

assert_contains() {
  local test_name="$1"
  local substring="$2"
  local actual="$3"
  if [[ "$actual" == *"$substring"* ]]; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    ERRORS="${ERRORS}\n  FAIL: ${test_name}\n    expected to contain: '${substring}'\n    actual: '${actual}'"
    echo "  FAIL: ${test_name}"
  fi
}

run() {
  $CLI "$@" 2>&1
}

# Clean slate
run FLUSHALL > /dev/null

echo "=== 32-BIT (R.*) COMMANDS ==="

# -------------------------------------------------------
echo "--- SETBIT / GETBIT ---"
assert_eq "SETBIT returns 0 for new bit" "0" "$(run R.SETBIT k1 10 1)"
assert_eq "SETBIT returns 1 for already set bit" "1" "$(run R.SETBIT k1 10 1)"
assert_eq "GETBIT returns 1 for set bit" "1" "$(run R.GETBIT k1 10)"
assert_eq "GETBIT returns 0 for unset bit" "0" "$(run R.GETBIT k1 999)"
assert_eq "GETBIT returns 0 for nonexistent key" "0" "$(run R.GETBIT nonexist 0)"
assert_eq "SETBIT can clear a bit" "1" "$(run R.SETBIT k1 10 0)"
assert_eq "GETBIT after clear" "0" "$(run R.GETBIT k1 10)"

# -------------------------------------------------------
echo "--- GETBITS ---"
run R.SETBIT k2 1 1 > /dev/null
run R.SETBIT k2 3 1 > /dev/null
run R.SETBIT k2 5 1 > /dev/null
result=$(run R.GETBITS k2 1 2 3 4 5)
expected=$(printf "1\n0\n1\n0\n1")
assert_eq "GETBITS multi" "$expected" "$result"
result=$(run R.GETBITS nonexist 1 2 3)
assert_eq "GETBITS nonexistent key is empty array" "" "$result"

# -------------------------------------------------------
echo "--- CLEARBITS ---"
run R.SETINTARRAY k3 1 2 3 4 5 > /dev/null
assert_eq "CLEARBITS default replies OK" "OK" "$(run R.CLEARBITS k3 1)"
assert_eq "CLEARBITS COUNT replies count" "2" "$(run R.CLEARBITS k3 3 5 99 COUNT)"
result=$(run R.GETINTARRAY k3)
expected=$(printf "2\n4")
assert_eq "CLEARBITS remaining values" "$expected" "$result"
assert_eq "CLEARBITS on nonexistent key is null" "" "$(run R.CLEARBITS nonexist 1 2)"

# -------------------------------------------------------
echo "--- CLEAR ---"
run R.SETINTARRAY k4 10 20 30 > /dev/null
assert_eq "CLEAR returns old cardinality" "3" "$(run R.CLEAR k4)"
assert_eq "BITCOUNT after CLEAR" "0" "$(run R.BITCOUNT k4)"
assert_eq "CLEAR nonexistent returns null" "" "$(run R.CLEAR nonexist)"

# -------------------------------------------------------
echo "--- SETINTARRAY / GETINTARRAY ---"
run R.SETINTARRAY k5 50 10 30 > /dev/null
result=$(run R.GETINTARRAY k5)
expected=$(printf "10\n30\n50")
assert_eq "SETINTARRAY + GETINTARRAY sorted" "$expected" "$result"
result=$(run R.GETINTARRAY nonexist)
assert_eq "GETINTARRAY nonexistent is empty" "" "$result"

# -------------------------------------------------------
echo "--- APPENDINTARRAY ---"
run R.SETINTARRAY k6 1 2 3 > /dev/null
run R.APPENDINTARRAY k6 4 5 > /dev/null
result=$(run R.GETINTARRAY k6)
expected=$(printf "1\n2\n3\n4\n5")
assert_eq "APPENDINTARRAY adds values" "$expected" "$result"
# Append to nonexistent key
run R.APPENDINTARRAY k6new 10 20 > /dev/null
result=$(run R.GETINTARRAY k6new)
expected=$(printf "10\n20")
assert_eq "APPENDINTARRAY creates key" "$expected" "$result"

# -------------------------------------------------------
echo "--- DELETEINTARRAY ---"
run R.SETINTARRAY k7 1 2 3 4 5 > /dev/null
run R.DELETEINTARRAY k7 2 4 > /dev/null
result=$(run R.GETINTARRAY k7)
expected=$(printf "1\n3\n5")
assert_eq "DELETEINTARRAY removes values" "$expected" "$result"

# -------------------------------------------------------
echo "--- RANGEINTARRAY ---"
# start/end are 0-based POSITIONS in the sorted array (pagination), not values
run R.SETINTARRAY k8 5 10 15 20 25 30 > /dev/null
result=$(run R.RANGEINTARRAY k8 1 4)
expected=$(printf "10\n15\n20\n25")
assert_eq "RANGEINTARRAY paginates by position" "$expected" "$result"
result=$(run R.RANGEINTARRAY k8 4 100)
expected=$(printf "25\n30")
assert_eq "RANGEINTARRAY truncates at cardinality" "$expected" "$result"
result=$(run R.RANGEINTARRAY k8 100 200)
assert_eq "RANGEINTARRAY past the end" "" "$result"
result=$(run R.RANGEINTARRAY nonexist 0 100)
assert_eq "RANGEINTARRAY nonexistent key" "" "$result"
assert_contains "RANGEINTARRAY range cap" "range too large" "$(run R.RANGEINTARRAY k8 0 200000000)"

# -------------------------------------------------------
echo "--- SETBITARRAY / GETBITARRAY ---"
run R.SETBITARRAY k9 "01010" > /dev/null
result=$(run R.GETINTARRAY k9)
expected=$(printf "1\n3")
assert_eq "SETBITARRAY parses bit string" "$expected" "$result"
result=$(run R.GETBITARRAY k9)
# SETBITARRAY "01010" sets bits {1,3}. GETBITARRAY returns [0..max] = "0101" (max=3)
assert_eq "GETBITARRAY returns bit string" "0101" "$result"
result=$(run R.GETBITARRAY nonexist)
assert_eq "GETBITARRAY nonexistent is empty" "" "$result"

# -------------------------------------------------------
echo "--- SETRANGE ---"
run R.SETRANGE k10 5 10 > /dev/null
assert_eq "SETRANGE cardinality" "5" "$(run R.BITCOUNT k10)"
assert_eq "SETRANGE min" "5" "$(run R.MIN k10)"
assert_eq "SETRANGE max (end-exclusive)" "9" "$(run R.MAX k10)"
# Error case
result=$(run R.SETRANGE k10err 10 5)
assert_contains "SETRANGE end < start" "ERR" "$result"

# -------------------------------------------------------
echo "--- SETFULL ---"
run R.SETFULL kfull > /dev/null
assert_eq "SETFULL GETBIT 0" "1" "$(run R.GETBIT kfull 0)"
assert_eq "SETFULL GETBIT max" "1" "$(run R.GETBIT kfull 4294967295)"
# SETFULL on existing key should error
result=$(run R.SETFULL kfull)
assert_contains "SETFULL existing key errors" "Roaring: key already exist" "$result"

# -------------------------------------------------------
echo "--- BITCOUNT ---"
run R.SETINTARRAY kcount 1 2 3 4 5 > /dev/null
assert_eq "BITCOUNT" "5" "$(run R.BITCOUNT kcount)"
assert_eq "BITCOUNT nonexistent" "0" "$(run R.BITCOUNT nonexist)"

# -------------------------------------------------------
echo "--- BITPOS ---"
run R.SETINTARRAY kpos 5 10 15 > /dev/null
assert_eq "BITPOS first set bit" "5" "$(run R.BITPOS kpos 1)"
assert_eq "BITPOS first unset bit" "0" "$(run R.BITPOS kpos 0)"
assert_eq "BITPOS nonexistent key bit=1" "-1" "$(run R.BITPOS nonexist 1)"
assert_eq "BITPOS nonexistent key bit=0" "0" "$(run R.BITPOS nonexist 0)"

# -------------------------------------------------------
echo "--- MIN / MAX ---"
run R.SETINTARRAY kminmax 100 200 300 > /dev/null
assert_eq "MIN" "100" "$(run R.MIN kminmax)"
assert_eq "MAX" "300" "$(run R.MAX kminmax)"
assert_eq "MIN nonexistent" "-1" "$(run R.MIN nonexist)"
assert_eq "MAX nonexistent" "-1" "$(run R.MAX nonexist)"

# -------------------------------------------------------
echo "--- OPTIMIZE ---"
run R.SETINTARRAY kopt 1 2 3 > /dev/null
assert_eq "OPTIMIZE returns OK" "OK" "$(run R.OPTIMIZE kopt)"

# -------------------------------------------------------
echo "--- CONTAINS ---"
run R.SETINTARRAY ca 1 2 3 4 5 > /dev/null
run R.SETINTARRAY cb 2 3 > /dev/null
run R.SETINTARRAY cc 1 2 3 4 5 > /dev/null
run R.SETINTARRAY cd 99 > /dev/null
assert_eq "CONTAINS default (overlap)" "1" "$(run R.CONTAINS ca cb)"
assert_eq "CONTAINS default (no overlap)" "0" "$(run R.CONTAINS ca cd)"
assert_contains "CONTAINS explicit NONE rejected" "invalid mode" "$(run R.CONTAINS ca cb NONE)"
assert_eq "CONTAINS ALL (subset)" "1" "$(run R.CONTAINS ca cb ALL)"
assert_eq "CONTAINS ALL (not subset)" "0" "$(run R.CONTAINS cb ca ALL)"
assert_eq "CONTAINS ALL_STRICT (proper subset)" "1" "$(run R.CONTAINS ca cb ALL_STRICT)"
assert_eq "CONTAINS ALL_STRICT (equal)" "0" "$(run R.CONTAINS ca cc ALL_STRICT)"
assert_eq "CONTAINS EQ" "1" "$(run R.CONTAINS ca cc EQ)"
assert_eq "CONTAINS EQ (not equal)" "0" "$(run R.CONTAINS ca cb EQ)"
# Error: nonexistent key
result=$(run R.CONTAINS ca nonexist)
assert_contains "CONTAINS nonexistent key errors" "Roaring: key does not exist" "$result"

# -------------------------------------------------------
echo "--- JACCARD ---"
run R.SETINTARRAY ja 1 2 3 4 > /dev/null
run R.SETINTARRAY jb 3 4 5 6 > /dev/null
# intersection={3,4}=2, union={1,2,3,4,5,6}=6 → 2/6 = 0.333...
result=$(run R.JACCARD ja jb)
assert_contains "JACCARD" "0.333333" "$result"

# -------------------------------------------------------
echo "--- DIFF ---"
run R.SETINTARRAY da 1 2 3 4 5 > /dev/null
run R.SETINTARRAY db 3 4 > /dev/null
run R.DIFF ddest da db
result=$(run R.GETINTARRAY ddest)
expected=$(printf "1\n2\n5")
assert_eq "DIFF result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP AND ---"
run R.SETINTARRAY ba 1 2 3 4 5 > /dev/null
run R.SETINTARRAY bb 3 4 5 6 7 > /dev/null
assert_eq "BITOP AND cardinality" "3" "$(run R.BITOP AND bdest ba bb)"
result=$(run R.GETINTARRAY bdest)
expected=$(printf "3\n4\n5")
assert_eq "BITOP AND result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP OR ---"
assert_eq "BITOP OR cardinality" "7" "$(run R.BITOP OR bodest ba bb)"
result=$(run R.GETINTARRAY bodest)
expected=$(printf "1\n2\n3\n4\n5\n6\n7")
assert_eq "BITOP OR result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP XOR ---"
assert_eq "BITOP XOR cardinality" "4" "$(run R.BITOP XOR bxdest ba bb)"
result=$(run R.GETINTARRAY bxdest)
expected=$(printf "1\n2\n6\n7")
assert_eq "BITOP XOR result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP NOT ---"
run R.SETINTARRAY bn 2 5 > /dev/null
# NOT with max=5 should flip [0,6) → {0,1,3,4}
assert_eq "BITOP NOT cardinality" "4" "$(run R.BITOP NOT bndest bn)"
result=$(run R.GETINTARRAY bndest)
expected=$(printf "0\n1\n3\n4")
assert_eq "BITOP NOT result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP ANDOR ---"
# ANDOR: (src[1] | src[2] | ...) & src[0]
run R.SETINTARRAY ao0 1 2 3 4 5 > /dev/null
run R.SETINTARRAY ao1 3 4 6 > /dev/null
run R.SETINTARRAY ao2 5 7 > /dev/null
# (ao1 | ao2) = {3,4,5,6,7} & ao0={1,2,3,4,5} → {3,4,5}
assert_eq "BITOP ANDOR cardinality" "3" "$(run R.BITOP ANDOR aodest ao0 ao1 ao2)"
result=$(run R.GETINTARRAY aodest)
expected=$(printf "3\n4\n5")
assert_eq "BITOP ANDOR result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP DIFF (ANDNOT) ---"
# DIFF: src[0] - src[1] - src[2]
run R.SETINTARRAY ad0 1 2 3 4 5 > /dev/null
run R.SETINTARRAY ad1 2 3 > /dev/null
run R.SETINTARRAY ad2 4 > /dev/null
assert_eq "BITOP DIFF cardinality" "2" "$(run R.BITOP DIFF addest ad0 ad1 ad2)"
result=$(run R.GETINTARRAY addest)
expected=$(printf "1\n5")
assert_eq "BITOP DIFF result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP DIFF1 (ORNOT) ---"
# DIFF1: (src[1] | src[2]) - src[0]
run R.SETINTARRAY d1a 3 4 > /dev/null
run R.SETINTARRAY d1b 1 2 3 > /dev/null
run R.SETINTARRAY d1c 4 5 6 > /dev/null
# (d1b | d1c) = {1,2,3,4,5,6} - d1a={3,4} → {1,2,5,6}
assert_eq "BITOP DIFF1 cardinality" "4" "$(run R.BITOP DIFF1 d1dest d1a d1b d1c)"
result=$(run R.GETINTARRAY d1dest)
expected=$(printf "1\n2\n5\n6")
assert_eq "BITOP DIFF1 result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP ONE ---"
# ONE: bits in exactly one source
run R.SETINTARRAY o1 1 2 3 > /dev/null
run R.SETINTARRAY o2 2 3 4 > /dev/null
run R.SETINTARRAY o3 3 4 5 > /dev/null
# 1 appears in 1 source, 2 in 2, 3 in 3, 4 in 2, 5 in 1 → {1, 5}
assert_eq "BITOP ONE cardinality" "2" "$(run R.BITOP ONE odest o1 o2 o3)"
result=$(run R.GETINTARRAY odest)
expected=$(printf "1\n5")
assert_eq "BITOP ONE result" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP with nonexistent source ---"
run R.SETINTARRAY be 1 2 3 > /dev/null
assert_eq "BITOP AND with empty source" "0" "$(run R.BITOP AND bedest be nonexist)"

# -------------------------------------------------------
echo "--- EXPORT / IMPORT via Lua ---"
run R.SETINTARRAY exp 10 20 30 > /dev/null
result=$(run EVAL "
local data = redis.call('R.EXPORT', 'exp')
redis.call('R.IMPORT', 'imp', data)
return redis.call('R.BITCOUNT', 'imp')
" 0)
assert_eq "EXPORT/IMPORT round-trip cardinality" "3" "$result"
result=$(run EVAL "
local data = redis.call('R.EXPORT', 'exp')
redis.call('R.IMPORT', 'imp', data)
return redis.call('R.GETINTARRAY', 'imp')
" 0)
expected=$(printf "10\n20\n30")
assert_eq "EXPORT/IMPORT values match" "$expected" "$result"

# Test IMPORT merge (OR)
result=$(run EVAL "
redis.call('R.SETINTARRAY', 'imp_a', 1, 2, 3)
redis.call('R.SETINTARRAY', 'imp_b', 3, 4, 5)
local data = redis.call('R.EXPORT', 'imp_b')
local card = redis.call('R.IMPORT', 'imp_a', data)
return card
" 0)
assert_eq "IMPORT OR-merge cardinality" "5" "$result"

# EXPORT nonexistent key
result=$(run R.EXPORT nonexist)
assert_contains "EXPORT nonexistent errors" "Roaring: key does not exist" "$result"

# -------------------------------------------------------
echo "--- STAT ---"
run R.SETINTARRAY ks 1 2 3 > /dev/null
result=$(run R.STAT ks)
assert_contains "STAT contains cardinality" "cardinality: 3" "$result"
assert_contains "STAT contains type" "type: bitmap" "$result"
result=$(run R.STAT ks JSON)
assert_contains "STAT JSON has type" "\"type\":\"bitmap\"" "$result"
assert_contains "STAT JSON has cardinality" "\"cardinality\":\"3\"" "$result"
# STAT nonexistent key
result=$(run R.STAT nonexist)
assert_eq "STAT nonexistent returns null" "" "$result"

# -------------------------------------------------------
echo "--- WRONGTYPE errors ---"
run SET stringkey "hello" > /dev/null
result=$(run R.GETBIT stringkey 0)
assert_contains "WRONGTYPE on string key" "wrong" "$result"

# -------------------------------------------------------
echo "--- Arity errors ---"
result=$(run R.SETBIT k1)
assert_contains "SETBIT wrong arity" "ERR" "$result"
result=$(run R.GETBIT)
assert_contains "GETBIT wrong arity" "ERR" "$result"

# -------------------------------------------------------
echo ""
echo "=== 64-BIT (R64.*) COMMANDS ==="
run FLUSHALL > /dev/null

echo "--- R64 SETBIT / GETBIT ---"
assert_eq "R64.SETBIT" "0" "$(run R64.SETBIT k64 4294967296 1)"
assert_eq "R64.GETBIT set" "1" "$(run R64.GETBIT k64 4294967296)"
assert_eq "R64.GETBIT unset" "0" "$(run R64.GETBIT k64 0)"
assert_eq "R64.GETBIT nonexistent" "0" "$(run R64.GETBIT nonexist 0)"

echo "--- R64 BITCOUNT / MIN / MAX ---"
run R64.SETBIT k64b 100 1 > /dev/null
run R64.SETBIT k64b 5000000000 1 > /dev/null
assert_eq "R64.BITCOUNT" "2" "$(run R64.BITCOUNT k64b)"
assert_eq "R64.MIN" "100" "$(run R64.MIN k64b)"
assert_eq "R64.MAX" "5000000000" "$(run R64.MAX k64b)"

echo "--- R64 SETINTARRAY / GETINTARRAY ---"
run R64.SETINTARRAY k64c 1 5000000000 10000000000 > /dev/null
result=$(run R64.GETINTARRAY k64c)
expected=$(printf "1\n5000000000\n10000000000")
assert_eq "R64 SETINTARRAY/GETINTARRAY" "$expected" "$result"

echo "--- R64 BITOP OR ---"
run R64.SETINTARRAY k64d 1 2 > /dev/null
run R64.SETINTARRAY k64e 2 3 > /dev/null
assert_eq "R64 BITOP OR cardinality" "3" "$(run R64.BITOP OR k64dest k64d k64e)"
result=$(run R64.GETINTARRAY k64dest)
expected=$(printf "1\n2\n3")
assert_eq "R64 BITOP OR result" "$expected" "$result"

echo "--- R64 EXPORT/IMPORT via Lua ---"
run R64.SETINTARRAY k64exp 1 5000000000 > /dev/null
result=$(run EVAL "
local data = redis.call('R64.EXPORT', 'k64exp')
redis.call('R64.IMPORT', 'k64imp', data)
return redis.call('R64.BITCOUNT', 'k64imp')
" 0)
assert_eq "R64 EXPORT/IMPORT round-trip" "2" "$result"

echo "--- R64 STAT ---"
result=$(run R.STAT k64exp)
assert_contains "R.STAT on R64 key" "type: bitmap64" "$result"
assert_contains "R.STAT on R64 cardinality" "cardinality: 2" "$result"
assert_contains "R.STAT on R64 container breakdown" "number of containers" "$result"
result=$(run R.STAT k64exp JSON)
assert_contains "R.STAT JSON on R64 containers" "\"array_container\"" "$result"

echo "--- R64 CONTAINS ---"
run R64.SETINTARRAY k64f 1 2 3 4 5 > /dev/null
run R64.SETINTARRAY k64g 2 3 > /dev/null
assert_eq "R64 CONTAINS ALL" "1" "$(run R64.CONTAINS k64f k64g ALL)"
assert_eq "R64 CONTAINS EQ" "0" "$(run R64.CONTAINS k64f k64g EQ)"

echo "--- R64 DIFF ---"
run R64.SETINTARRAY k64h 1 2 3 4 5 > /dev/null
run R64.SETINTARRAY k64i 3 4 > /dev/null
run R64.DIFF k64j k64h k64i
result=$(run R64.GETINTARRAY k64j)
expected=$(printf "1\n2\n5")
assert_eq "R64 DIFF result" "$expected" "$result"

# -------------------------------------------------------
echo ""
echo "=== EDGE CASES AND COMPATIBILITY ==="
run FLUSHALL > /dev/null

# -------------------------------------------------------
echo "--- BITOP NOT with optional last arg ---"
run R.SETINTARRAY notsrc 1 3 > /dev/null
assert_eq "NOT with last=5 cardinality" "4" "$(run R.BITOP NOT notdest notsrc 5)"
result=$(run R.GETINTARRAY notdest)
expected=$(printf "0\n2\n4\n5")
assert_eq "NOT with last=5 values" "$expected" "$result"
assert_eq "NOT last below max is raised to max" "2" "$(run R.BITOP NOT notdest2 notsrc 2)"
result=$(run R.GETINTARRAY notdest2)
expected=$(printf "0\n2")
assert_eq "NOT raised-last values" "$expected" "$result"
assert_contains "NOT with too many args errors" "wrong number" "$(run R.BITOP NOT d s 5 extra)"

run R64.SETINTARRAY notsrc64 1 3 > /dev/null
assert_eq "R64 NOT with last=5 cardinality" "4" "$(run R64.BITOP NOT notdest64 notsrc64 5)"
result=$(run R64.GETINTARRAY notdest64)
expected=$(printf "0\n2\n4\n5")
assert_eq "R64 NOT with last=5 values" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP NOT on empty/missing source ---"
assert_eq "NOT missing source cardinality" "0" "$(run R.BITOP NOT notempty missingkey)"
assert_eq "NOT missing source creates key" "vrroaring" "$(run TYPE notempty)"
assert_eq "NOT missing source bitcount" "0" "$(run R.BITCOUNT notempty)"
assert_eq "R64 NOT missing source cardinality" "0" "$(run R64.BITOP NOT notempty64 missingkey64)"
assert_eq "R64 NOT missing source creates key" "vroarng64" "$(run TYPE notempty64)"
assert_eq "NOT missing source with last fills range" "4" "$(run R.BITOP NOT notfull missingkey 3)"
result=$(run R.GETINTARRAY notfull)
expected=$(printf "0\n1\n2\n3")
assert_eq "NOT missing source with last values" "$expected" "$result"
run R.SETINTARRAY notow 9 > /dev/null
run R.BITOP NOT notow missingkey > /dev/null
assert_eq "NOT overwrites existing dest with empty result" "0" "$(run R.BITCOUNT notow)"

# -------------------------------------------------------
echo "--- upstream parity: BITOP arity, SETRANGE exclusivity, CLEARBITS ---"
assert_contains "variadic BITOP needs two sources" "wrong number" "$(run R.BITOP AND ssdest ssa)"
run R.SETRANGE sre 5 8 > /dev/null
result=$(run R.GETINTARRAY sre)
expected=$(printf "5\n6\n7")
assert_eq "SETRANGE is end-exclusive" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP getkeys (cluster routing) ---"
result=$(run COMMAND GETKEYS R.BITOP NOT gd gs 100)
expected=$(printf "gd\ngs")
assert_eq "GETKEYS NOT excludes last arg" "$expected" "$result"
result=$(run COMMAND GETKEYS R.BITOP AND gd ga gb gc)
expected=$(printf "gd\nga\ngb\ngc")
assert_eq "GETKEYS variadic reports all keys" "$expected" "$result"
result=$(run COMMAND GETKEYS R64.BITOP NOT gd gs 100)
expected=$(printf "gd\ngs")
assert_eq "R64 GETKEYS NOT excludes last arg" "$expected" "$result"

# -------------------------------------------------------
echo "--- BITOP invalid operation error reply ---"
assert_contains "BITOP invalid op is an error" "syntax error" "$(run R.BITOP FOO d s1 s2)"
assert_contains "R64 BITOP invalid op is an error" "syntax error" "$(run R64.BITOP BAR d s1 s2)"

# -------------------------------------------------------
echo "--- CLEARBITS duplicate offsets ---"
run R.SETINTARRAY dupck 5 7 > /dev/null
assert_eq "CLEARBITS duplicate offsets count once" "1" "$(run R.CLEARBITS dupck 5 5 5 COUNT)"
result=$(run R.GETINTARRAY dupck)
assert_eq "CLEARBITS duplicates leave other bits" "7" "$result"
run R64.SETINTARRAY dupck64 5 7 > /dev/null
assert_eq "R64 CLEARBITS duplicate offsets count once" "1" "$(run R64.CLEARBITS dupck64 5 5 5 COUNT)"

# -------------------------------------------------------
echo "--- DELETEINTARRAY duplicate deletes of last value ---"
run R64.SETINTARRAY dupdel64 100 > /dev/null
assert_eq "R64 DELETEINTARRAY duplicate deletes OK" "OK" "$(run R64.DELETEINTARRAY dupdel64 100 100 100)"
assert_eq "R64 DELETEINTARRAY duplicate deletes result" "0" "$(run R64.BITCOUNT dupdel64)"
run R.SETINTARRAY dupdel 100 > /dev/null
assert_eq "DELETEINTARRAY duplicate deletes OK" "OK" "$(run R.DELETEINTARRAY dupdel 100 100 100)"
assert_eq "DELETEINTARRAY duplicate deletes result" "0" "$(run R.BITCOUNT dupdel)"

# -------------------------------------------------------
echo "--- BITPOS edge cases ---"
run R.SETBIT bpz 0 1 > /dev/null
assert_eq "BITPOS 0 on {0} bitmap" "1" "$(run R.BITPOS bpz 0)"
assert_eq "BITPOS 1 on missing key" "-1" "$(run R.BITPOS bpmissing 1)"
assert_eq "BITPOS 0 on missing key" "0" "$(run R.BITPOS bpmissing 0)"
run R64.SETBIT bpz64 0 1 > /dev/null
assert_eq "R64 BITPOS 0 on {0} bitmap" "1" "$(run R64.BITPOS bpz64 0)"
assert_eq "R64 BITPOS 1 on missing key" "-1" "$(run R64.BITPOS bpmissing64 1)"
assert_eq "R64 BITPOS 0 on missing key" "0" "$(run R64.BITPOS bpmissing64 0)"
run R.SETINTARRAY bpc 3 > /dev/null
run R.CLEAR bpc > /dev/null
assert_eq "BITPOS 0 on existing empty bitmap" "0" "$(run R.BITPOS bpc 0)"
assert_eq "BITPOS 1 on existing empty bitmap" "-1" "$(run R.BITPOS bpc 1)"

# -------------------------------------------------------
echo "--- Full u64 range (parse + reply) ---"
assert_eq "SETBIT above i64::MAX" "0" "$(run R64.SETBIT bigu64 9223372036854775808 1)"
assert_eq "GETBIT above i64::MAX" "1" "$(run R64.GETBIT bigu64 9223372036854775808)"
assert_eq "MAX above i64::MAX replies decimal string" "9223372036854775808" "$(run R64.MAX bigu64)"
assert_eq "GETINTARRAY above i64::MAX" "9223372036854775808" "$(run R64.GETINTARRAY bigu64)"

# -------------------------------------------------------
echo "--- RANGEINTARRAY inverted range (crash guard) ---"
run R.SETINTARRAY rir 1 2 3 > /dev/null
assert_eq "RANGEINTARRAY inverted range replies empty" "" "$(run R.RANGEINTARRAY rir 5 2)"
assert_eq "RANGEINTARRAY server alive after inverted range" "PONG" "$(run PING)"
run R64.SETINTARRAY rir64 1 2 3 > /dev/null
assert_eq "R64 RANGEINTARRAY inverted range replies empty" "" "$(run R64.RANGEINTARRAY rir64 5 2)"

# -------------------------------------------------------
echo "--- GETBITARRAY huge-max guard ---"
run R.SETBIT gba 4000000000 1 > /dev/null
assert_contains "GETBITARRAY huge max is an error" "range too large" "$(run R.GETBITARRAY gba)"
assert_eq "GETBITARRAY server alive after huge max" "PONG" "$(run PING)"

# -------------------------------------------------------
echo "--- COPY on module keys ---"
run R.SETINTARRAY cpk 1 2 3 > /dev/null
assert_eq "COPY module key" "1" "$(run COPY cpk cpk2)"
assert_eq "COPY result equal" "1" "$(run R.CONTAINS cpk cpk2 EQ)"
run R.SETBIT cpk2 99 1 > /dev/null
assert_eq "COPY is deep (independent)" "0" "$(run R.GETBIT cpk 99)"
run R64.SETINTARRAY cpk64 5 5000000000 > /dev/null
assert_eq "COPY r64 module key" "1" "$(run COPY cpk64 cpk64b)"
assert_eq "COPY r64 result equal" "1" "$(run R64.CONTAINS cpk64 cpk64b EQ)"

# -------------------------------------------------------
echo "--- R64.OPTIMIZE (roaring 0.11.4+) ---"
run R64.SETRANGE optr64 0 100000 > /dev/null
assert_eq "R64 OPTIMIZE returns OK" "OK" "$(run R64.OPTIMIZE optr64)"
assert_eq "R64 OPTIMIZE preserves data" "100000" "$(run R64.BITCOUNT optr64)"

# -------------------------------------------------------
echo "--- Streamed and paged replies on multi-container keys ---"
# 300k values spread over five containers; pages deep into the key walk from
# one select, and full replies are streamed element by element.
run R.SETRANGE pg 0 100000 > /dev/null
run R.SETRANGE pg 200000 400000 > /dev/null
run R64.SETRANGE pg64 0 100000 > /dev/null
run R64.SETRANGE pg64 200000 400000 > /dev/null
for prefix in R R64; do
  key=pg
  if [ "$prefix" = R64 ]; then key=pg64; fi
  expected=$(printf "99999\n200000\n200001")
  assert_eq "$prefix RANGEINTARRAY page across a gap" "$expected" "$(run $prefix.RANGEINTARRAY $key 99999 100001)"
  expected=$(printf "399998\n399999")
  assert_eq "$prefix RANGEINTARRAY page truncated at the end" "$expected" "$(run $prefix.RANGEINTARRAY $key 299998 400000)"
  assert_eq "$prefix RANGEINTARRAY page at cardinality" "" "$(run $prefix.RANGEINTARRAY $key 300000 300005)"
  assert_eq "$prefix GETINTARRAY streams every value" "300000 399999" \
    "$(run EVAL "local a = redis.call('$prefix.GETINTARRAY', KEYS[1]) return #a .. ' ' .. a[#a]" 1 $key)"
  assert_eq "$prefix GETBITS streams one reply per offset" "1 0 1 0" \
    "$(run $prefix.GETBITS $key 0 100000 399999 400000 | tr '\n' ' ' | sed 's/ $//')"
  assert_eq "$prefix BITPOS 0 skips a long run" "100000" "$(run $prefix.BITPOS $key 0)"
done

# -------------------------------------------------------
echo "--- Set operations: aliasing and memory ---"
run R.SETINTARRAY al_a 1 2 > /dev/null
run R.SETINTARRAY al_b 3 > /dev/null
assert_eq "BITOP dest may be a source" "3" "$(run R.BITOP OR al_a al_a al_b)"
assert_eq "BITOP aliased result" "$(printf "1\n2\n3")" "$(run R.GETINTARRAY al_a)"
assert_eq "DIFF dest may be a source" "OK" "$(run R.DIFF al_a al_a al_b)"
assert_eq "DIFF aliased result" "$(printf "1\n2")" "$(run R.GETINTARRAY al_a)"
# A small AND of two large keys must not keep its inputs' capacity: 20k
# values each over ~30 containers, overlapping in 200.
# (Lua's unpack takes at most ~8000 values, so append in chunks.)
build_lua="local t = {} for i = 1, 20000 do t[#t + 1] = i * 97 + ARGV[1] * ((i % 100 == 0) and 0 or 1)
  if #t == 5000 then redis.call('R.APPENDINTARRAY', KEYS[1], unpack(t)) t = {} end end return 1"
run EVAL "$build_lua" 1 big_a 0 > /dev/null
run EVAL "$build_lua" 1 big_b 1 > /dev/null
assert_eq "large AND sources built" "$(printf "20000\n20000")" "$(run R.BITCOUNT big_a; run R.BITCOUNT big_b)"
assert_eq "AND of large keys" "200" "$(run R.BITOP AND and_small big_a big_b)"
mem=$(run MEMORY USAGE and_small)
assert_eq "small AND result is trimmed (MEMORY USAGE $mem < 10000)" "1" "$([ "$mem" -lt 10000 ] && echo 1 || echo 0)"

# -------------------------------------------------------
echo "--- EXPORT is canonical across histories ---"
# {5,6,7}: run and array encodings are the same size, and SETRANGE builds a
# run container while SETINTARRAY builds an array one.
for prefix in R R64; do
  result=$(run EVAL "
redis.call('$prefix.SETRANGE', KEYS[1], 5, 8)
redis.call('$prefix.SETINTARRAY', KEYS[2], 5, 6, 7)
if redis.call('$prefix.EXPORT', KEYS[1]) == redis.call('$prefix.EXPORT', KEYS[2]) then return 1 end
return 0" 2 canon_run_$prefix canon_arr_$prefix)
  assert_eq "$prefix EXPORT identical for run- and array-built {5,6,7}" "1" "$result"
done

# -------------------------------------------------------
echo "--- R64.IMPORT of a blob with an empty sub-bitmap ---"
# {1} under high word 0 plus an empty sub-bitmap under 7 (the format allows
# it): the key must still equal {1} built any other way.
result=$(run EVAL "
local blob = string.char(2,0,0,0,0,0,0,0, 0,0,0,0, 0x3A,0x30,0,0, 1,0,0,0, 0,0,0,0, 16,0,0,0, 1,0,
  7,0,0,0, 0x3A,0x30,0,0, 0,0,0,0)
redis.call('R64.IMPORT', KEYS[1], blob)
redis.call('R64.SETINTARRAY', KEYS[2], 1)
return {redis.call('R64.CONTAINS', KEYS[1], KEYS[2], 'EQ'), redis.call('R64.CONTAINS', KEYS[2], KEYS[1], 'ALL')}
" 2 imp_empty64 plain64)
assert_eq "R64.IMPORT empty sub-bitmap: EQ and ALL hold" "$(printf "1\n1")" "$result"

# -------------------------------------------------------
echo "--- SETBITARRAY reads raw bytes (upstream parity) ---"
# Byte i == '1' sets bit i; a non-UTF-8 byte is just not '1'.
result=$(run EVAL "redis.call('R.SETBITARRAY', KEYS[1], string.char(255) .. '1') return redis.call('R.GETINTARRAY', KEYS[1])" 1 sba_raw)
assert_eq "SETBITARRAY with a non-UTF-8 byte" "1" "$result"
result=$(run EVAL "redis.call('R64.SETBITARRAY', KEYS[1], string.char(200, 1) .. '01') return redis.call('R64.GETINTARRAY', KEYS[1])" 1 sba_raw64)
assert_eq "R64 SETBITARRAY with non-UTF-8 bytes" "3" "$result"

# -------------------------------------------------------
echo "--- R.SETFULL interplay ---"
run R.SETFULL qa_full > /dev/null
assert_eq "SETFULL: BITPOS 0 is -1" "-1" "$(run R.BITPOS qa_full 0)"
assert_eq "SETFULL: tail page" "$(printf "4294967293\n4294967294\n4294967295")" \
  "$(run R.RANGEINTARRAY qa_full 4294967293 4294967295)"
assert_eq "SETFULL: JACCARD with itself" "1" "$(run R.JACCARD qa_full qa_full)"
assert_eq "SETFULL: NOT is empty" "0" "$(run R.BITOP NOT qa_not qa_full)"
assert_contains "SETFULL: GETBITARRAY refused" "range too large" "$(run R.GETBITARRAY qa_full)"
run R.SETBIT qa_full 77 0 > /dev/null
assert_eq "SETFULL minus one bit: BITPOS 0" "77" "$(run R.BITPOS qa_full 0)"

# -------------------------------------------------------
echo "--- EXPORT is a read: TTL kept, allowed in read-only scripts ---"
run R.SETRANGE qa_ttl 5 8 > /dev/null
run EXPIRE qa_ttl 1000 > /dev/null
run R.EXPORT qa_ttl > /dev/null
assert_eq "EXPORT keeps the TTL" "1000" "$(run TTL qa_ttl)"
run R.OPTIMIZE qa_ttl > /dev/null
assert_eq "OPTIMIZE keeps the TTL" "1000" "$(run TTL qa_ttl)"
assert_eq "EVAL_RO may call EXPORT" "1" \
  "$(run EVAL_RO "if redis.call('R.EXPORT', KEYS[1]) then return 1 end return 0" 1 qa_ttl)"

# -------------------------------------------------------
echo "--- R64 paging across a 2^32 border ---"
run R64.SETRANGE qa_border 4294967290 4294967300 > /dev/null
assert_eq "R64 page straddles the border" "$(printf "4294967294\n4294967295\n4294967296\n4294967297")" \
  "$(run R64.RANGEINTARRAY qa_border 4 7)"
assert_eq "R64 BITPOS 0 after a border-crossing run" "0" "$(run R64.BITPOS qa_border 0)"
run R64.SETRANGE qa_border 0 4294967290 > /dev/null
assert_eq "R64 BITPOS 0 past a run across the border" "4294967300" "$(run R64.BITPOS qa_border 0)"

# -------------------------------------------------------
echo "--- CLEARBITS with only the COUNT flag ---"
run R.SETINTARRAY qa_cb 1 2 3 > /dev/null
assert_eq "CLEARBITS with only COUNT counts nothing (upstream parity)" "0" "$(run R.CLEARBITS qa_cb COUNT)"
assert_eq "and clears nothing" "3" "$(run R.BITCOUNT qa_cb)"

echo "=== SYSTEMATIC ERROR COVERAGE ==="
run FLUSHALL > /dev/null

# -------------------------------------------------------
echo "--- wrong arity: every command with no arguments ---"
ALL_COMMANDS="R.SETBIT R.GETBIT R.GETBITS R.CLEARBITS R.CLEAR R.SETINTARRAY R.GETINTARRAY \
R.APPENDINTARRAY R.DELETEINTARRAY R.RANGEINTARRAY R.SETBITARRAY R.GETBITARRAY R.SETRANGE \
R.SETFULL R.BITCOUNT R.BITPOS R.MIN R.MAX R.OPTIMIZE R.CONTAINS R.JACCARD R.DIFF R.BITOP \
R.EXPORT R.IMPORT R64.SETBIT R64.GETBIT R64.GETBITS R64.CLEARBITS R64.CLEAR R64.SETINTARRAY \
R64.GETINTARRAY R64.APPENDINTARRAY R64.DELETEINTARRAY R64.RANGEINTARRAY R64.SETBITARRAY \
R64.GETBITARRAY R64.SETRANGE R64.SETFULL R64.BITCOUNT R64.BITPOS R64.MIN R64.MAX R64.OPTIMIZE \
R64.CONTAINS R64.JACCARD R64.DIFF R64.BITOP R64.EXPORT R64.IMPORT R.STAT"
for cmd in $ALL_COMMANDS; do
  assert_contains "no-args arity error: $cmd" "wrong number of arguments" "$(run $cmd)"
done

# -------------------------------------------------------
echo "--- WRONGTYPE: every key command against a string key ---"
run SET plainstr hello > /dev/null
for prefix in R R64; do
  WRONGTYPE_CALLS=(
    "$prefix.GETBIT plainstr 0"
    "$prefix.SETBIT plainstr 0 1"
    "$prefix.GETBITS plainstr 1"
    "$prefix.CLEARBITS plainstr 1"
    "$prefix.CLEAR plainstr"
    "$prefix.SETINTARRAY plainstr 1"
    "$prefix.GETINTARRAY plainstr"
    "$prefix.APPENDINTARRAY plainstr 1"
    "$prefix.DELETEINTARRAY plainstr 1"
    "$prefix.RANGEINTARRAY plainstr 0 10"
    "$prefix.SETBITARRAY plainstr 01"
    "$prefix.GETBITARRAY plainstr"
    "$prefix.SETRANGE plainstr 0 5"
    "$prefix.SETFULL plainstr"
    "$prefix.BITCOUNT plainstr"
    "$prefix.BITPOS plainstr 1"
    "$prefix.MIN plainstr"
    "$prefix.MAX plainstr"
    "$prefix.OPTIMIZE plainstr"
    "$prefix.EXPORT plainstr"
    "$prefix.CONTAINS plainstr plainstr"
    "$prefix.JACCARD plainstr plainstr"
    "$prefix.DIFF wtdest plainstr plainstr"
    "$prefix.BITOP AND wtdest plainstr plainstr"
    "$prefix.BITOP NOT wtdest plainstr"
  )
  for call in "${WRONGTYPE_CALLS[@]}"; do
    assert_contains "WRONGTYPE: $call" "WRONGTYPE" "$(run $call)"
  done
done
assert_contains "WRONGTYPE: R.STAT plainstr" "WRONGTYPE" "$(run R.STAT plainstr)"
# BITOP with a wrong-type destination (sources valid)
run R.SETINTARRAY wtsrc 1 2 > /dev/null
assert_contains "WRONGTYPE: R.BITOP dest is string" "WRONGTYPE" "$(run R.BITOP OR plainstr wtsrc wtsrc)"

# -------------------------------------------------------
echo "--- semantic errors ---"
assert_contains "CONTAINS missing key" "key does not exist" "$(run R.CONTAINS nokey1 nokey2)"
assert_contains "JACCARD missing key" "key does not exist" "$(run R.JACCARD nokey1 nokey2)"
assert_contains "EXPORT missing key" "key does not exist" "$(run R.EXPORT nokey1)"
assert_contains "R64 CONTAINS missing key" "key does not exist" "$(run R64.CONTAINS nokey1 nokey2)"
run R.SETBIT fullkey 1 1 > /dev/null
assert_contains "SETFULL on existing key" "already exist" "$(run R.SETFULL fullkey)"
assert_contains "IMPORT with garbage binary" "bad binary" "$(run R.IMPORT importkey notaroaringblob)"
assert_contains "R64 IMPORT with garbage binary" "bad binary" "$(run R64.IMPORT importkey notaroaringblob)"
assert_contains "SETBIT non-numeric offset" "invalid" "$(run R.SETBIT badkey abc 1)"
assert_contains "SETBIT bit value out of range" "must be either 0 or 1" "$(run R.SETBIT badkey 1 2)"
assert_eq "SETBIT offset out of 32-bit range" "ERR invalid offset: must be an unsigned 32 bit integer" "$(run R.SETBIT badkey 4294967296 1)"
assert_contains "CONTAINS invalid mode" "invalid mode" "$(run R.CONTAINS wtsrc wtsrc BOGUS)"
assert_contains "SETRANGE inverted range" "must be >= start" "$(run R.SETRANGE rangekey 5 2)"

# -------------------------------------------------------
echo "=== GRAMMAR, CHECK ORDER, REPLY FORMATS ==="
# Exact replies, a compatibility contract (suite 09 of the testing repo
# checks the same cases byte for byte).
run FLUSHALL > /dev/null
run SET pstr x > /dev/null
run R.SETINTARRAY pr 1 2 3 100 > /dev/null
run R64.SETINTARRAY pr64 1 2 3 100 > /dev/null
run R.SETINTARRAY pe 1 > /dev/null
run R.CLEAR pe > /dev/null

echo "--- argument grammar ---"
# 32-bit values: "0" or a non-zero digit then digits, at most 4294967295.
# 64-bit values: an optional '+', digits (leading zeros allowed).
U32="ERR invalid offset: must be an unsigned 32 bit integer"
U64="ERR invalid offset: must be an unsigned 64 bit integer"
for v in +5 005 -1 " 5" "5 " 1e3 0x10 4294967296 ""; do
  assert_eq "R.GETBIT rejects '$v'" "$U32" "$(run R.GETBIT pr "$v")"
done
assert_eq "R64.GETBIT accepts +1" "1" "$(run R64.GETBIT pr64 +1)"
assert_eq "R64.GETBIT accepts 003" "1" "$(run R64.GETBIT pr64 003)"
for v in -1 " 5" 1e3 18446744073709551616 ""; do
  assert_eq "R64.GETBIT rejects '$v'" "$U64" "$(run R64.GETBIT pr64 "$v")"
done
assert_eq "SETBIT value must be exactly 0 or 1" "ERR invalid value: must be either 0 or 1" "$(run R.SETBIT pr 1 01)"
assert_eq "BITPOS bit must be exactly 0 or 1" "ERR invalid bit: must be either 0 or 1" "$(run R64.BITPOS pr64 +1)"
assert_eq "SETINTARRAY value grammar" "ERR invalid value: must be an unsigned 32 bit integer" "$(run R.SETINTARRAY psi 1 +2)"
assert_eq "  ... nothing stored" "0" "$(run EXISTS psi)"

echo "--- check order ---"
assert_eq "R.GETBIT answers 0 for a missing key before parsing" "0" "$(run R.GETBIT pmissing abc)"
assert_eq "R64.GETBIT parses before the missing-key answer" "$U64" "$(run R64.GETBIT pmissing abc)"
assert_contains "WRONGTYPE before argument errors (SETBIT)" "WRONGTYPE" "$(run R.SETBIT pstr abc 1)"
assert_contains "WRONGTYPE before argument errors (SETRANGE)" "WRONGTYPE" "$(run R64.SETRANGE pstr x y)"
assert_contains "WRONGTYPE before argument errors (RANGEINTARRAY)" "WRONGTYPE" "$(run R.RANGEINTARRAY pstr x y)"
assert_eq "GETBITS on a missing key: empty, offsets unparsed" "" "$(run R.GETBITS pmissing abc)"
assert_eq "CLEARBITS on a missing key: nil, offsets unparsed" "" "$(run R.CLEARBITS pmissing abc)"
assert_eq "DELETEINTARRAY on a missing key: created, values unparsed" "OK" "$(run R.DELETEINTARRAY pdel abc)"
assert_eq "  ... as an empty key" "0" "$(run R.BITCOUNT pdel)"
assert_contains "DIFF checks the destination type first" "WRONGTYPE" "$(run R.DIFF pstr pmissing pr)"
assert_eq "BITOP NOT parses last before key types" "ERR invalid last: must be an unsigned 32 bit integer" "$(run R.BITOP NOT pstr pr abc)"
assert_contains "variadic BITOP checks the destination first" "WRONGTYPE" "$(run R.BITOP AND pstr pr pmissing)"
assert_eq "R SETRANGE end before start" "ERR invalid end: must be >= start" "$(run R.SETRANGE psr 5 2)"
assert_eq "R64 SETRANGE end before start (upstream's wording)" "ERR invalid end: must >= start" "$(run R64.SETRANGE psr 5 2)"
assert_eq "OPTIMIZE requires the key" "Roaring: key does not exist" "$(run R.OPTIMIZE pmissing)"
assert_eq "R64.OPTIMIZE requires the key" "Roaring: key does not exist" "$(run R64.OPTIMIZE pmissing)"

echo "--- case-sensitive tokens ---"
assert_eq "BITOP operation names are exact" "ERR syntax error" "$(run R.BITOP and pd pr pr)"
assert_eq "BITOP NOT is exact" "ERR syntax error" "$(run R64.BITOP not pd pr64)"
assert_eq "CONTAINS modes are exact" "ERR invalid mode argument: all" "$(run R.CONTAINS pr pr all)"
assert_eq "CONTAINS rejects an explicit NONE" "ERR invalid mode argument: NONE" "$(run R.CONTAINS pr pr NONE)"
assert_eq "CONTAINS echoes a non-UTF-8 mode byte for byte" "255" "$(run EVAL "return string.byte(redis.pcall('R.CONTAINS', KEYS[1], KEYS[1], 'x\255').err, -1)" 1 pr)"
assert_eq "CLEARBITS takes only COUNT as the flag" "$U32" "$(run R.CLEARBITS pr 1 count)"
assert_contains "STAT takes only JSON for JSON" "type: bitmap" "$(run R.STAT pr json)"

echo "--- JACCARD ---"
run R.SETINTARRAY pq 1 2 > /dev/null
run R.SETINTARRAY pj 1 > /dev/null
run R.SETRANGE pj3 1 4 > /dev/null
assert_eq "JACCARD of two empty sets is -1" "-1" "$(run R.JACCARD pe pe)"
assert_eq "JACCARD of disjoint sets is 0" "0" "$(run R.JACCARD pe pr)"
assert_eq "JACCARD exact decimal" "0.5" "$(run R.JACCARD pr pq)"
assert_eq "JACCARD otherwise %.17g" "0.33333333333333331" "$(run R.JACCARD pj pj3)"
assert_eq "JACCARD is a bulk string (RESP2)" "string" "$(run EVAL "return type(redis.call('R.JACCARD', KEYS[1], KEYS[2]))" 2 pj pj3)"
assert_eq "JACCARD is a bulk string (RESP3)" "string" "$(run EVAL "redis.setresp(3); return type(redis.call('R.JACCARD', KEYS[1], KEYS[2]))" 2 pj pj3)"

echo "--- GETBITARRAY / STAT formats ---"
assert_eq "GETBITARRAY of an empty key is 0" "0" "$(run R.GETBITARRAY pe)"
assert_eq "GETBITARRAY of a missing key is a simple string" "table" "$(run EVAL "return type(redis.call('R.GETBITARRAY', KEYS[1]))" 1 pmissing)"
expected=$(printf 'type: bitmap\ncardinality: 4\nnumber of containers: 1\nmax value: 100\nmin value: 1\nnumber of array containers: 1\n\tarray container values: 4\n\tarray container bytes: 8\nbitset  containers: 0\n\tbitset  container values: 0\n\tbitset  container bytes: 0\nrun containers: 0\n\trun container values: 0\n\trun container bytes: 0')
assert_eq "STAT text in upstream's layout and units" "$expected" "$(run R.STAT pr)"
assert_contains "STAT of an empty key: min is the width's maximum" "min value: 4294967295" "$(run R.STAT pe)"
assert_contains "STAT on R64 keys" "type: bitmap64" "$(run R.STAT pr64)"
assert_eq "STAT is a verbatim txt string (RESP3)" "txt" "$(run EVAL "redis.setresp(3); return redis.call('R.STAT', KEYS[1]).verbatim_string.format" 1 pr)"

echo "--- 64-bit positions ---"
assert_eq "full-width R.RANGEINTARRAY lists the set" "$(printf '1\n2\n3\n100')" "$(run R.RANGEINTARRAY pr 0 4294967295)"
assert_contains "one short of full width is over the cap" "range too large" "$(run R.RANGEINTARRAY pr 0 4294967294)"
assert_eq "full-width R64.RANGEINTARRAY lists the set" "$(printf '1\n2\n3\n100')" "$(run R64.RANGEINTARRAY pr64 0 18446744073709551615)"
assert_eq "R64 positions past 2^63 are positions" "" "$(run R64.RANGEINTARRAY pr64 9223372036854775808 9223372036854775810)"
assert_eq "R64 window of exactly 100M positions is allowed" "100" "$(run R64.RANGEINTARRAY pr64 3 100000002)"
assert_contains "R64 window wider than the cap is refused" "range too large" "$(run R64.RANGEINTARRAY pr64 3 100000003)"

# -------------------------------------------------------
echo "=== LIMITS ==="
# Commands that would list or build more than the limits refuse up front.
R238="Roaring: range too large: maximum 274877906944 elements"
run R.SETFULL lfull > /dev/null
assert_eq "GETINTARRAY past 100M values refused" "Roaring: range too large: maximum 100000000 elements" "$(run R.GETINTARRAY lfull)"
assert_eq "R64.SETFULL refused" "$R238" "$(run R64.SETFULL lfull64)"
assert_eq "  ... nothing stored" "0" "$(run EXISTS lfull64)"
assert_eq "R64.SETRANGE past 2^38 values refused" "$R238" "$(run R64.SETRANGE lsr 0 274877906945)"
assert_eq "R64.SETRANGE of a full 32-bit sub-bitmap works" "OK" "$(run R64.SETRANGE lsr 4294967296 8589934592)"
assert_eq "  ... 2^32 values" "4294967296" "$(run R64.BITCOUNT lsr)"
assert_eq "R64.BITOP NOT up to 2^64 refused" "$R238" "$(run R64.BITOP NOT lnot lmissing 18446744073709551615)"
run R64.SETBIT lbig 9223372036854775808 1 > /dev/null
assert_eq "R64.BITOP NOT of a source past 2^38 refused" "$R238" "$(run R64.BITOP NOT lnot lbig)"
assert_eq "  ... destination untouched" "0" "$(run EXISTS lnot)"
assert_eq "R64.BITOP NOT over a full 32-bit universe works" "4294967296" "$(run R64.BITOP NOT lnot lmissing 4294967295)"
assert_eq "R.BITOP NOT up to 4294967295 works" "4294967292" "$(run R.BITOP NOT lnot32 pr 4294967295)"
assert_eq "server alive after the limits" "PONG" "$(run PING)"
run DEL lfull lsr lnot lnot32 lbig > /dev/null

# -------------------------------------------------------
echo "=== IMPORT VALIDATION ==="
# A blob must be exactly one valid bitmap: trailing bytes and 64-bit blobs
# whose high words do not strictly increase are refused, never truncated
# or merged.
BAD="ERR bad binary data for roaring"
run R.SETINTARRAY vsrc 1 2 70000 > /dev/null
assert_eq "IMPORT rejects trailing bytes" "$BAD" "$(run EVAL "return redis.pcall('R.IMPORT', KEYS[2], redis.call('R.EXPORT', KEYS[1]) .. '\0')" 2 vsrc vdst)"
assert_eq "  ... nothing stored" "0" "$(run EXISTS vdst)"
assert_eq "IMPORT accepts the exact blob" "3" "$(run EVAL "return redis.call('R.IMPORT', KEYS[2], redis.call('R.EXPORT', KEYS[1]))" 2 vsrc vdst)"
# 64-bit layout: u64 count, then per sub-bitmap a u32 high word and a 32-bit blob.
B64="local s = redis.call('R.EXPORT', KEYS[1]); local function hi(h) return string.char(h, 0, 0, 0) end; local two = string.char(2, 0, 0, 0, 0, 0, 0, 0)"
assert_eq "R64.IMPORT rejects a repeated high word" "$BAD" "$(run EVAL "$B64; return redis.pcall('R64.IMPORT', KEYS[2], two .. hi(0) .. s .. hi(0) .. s)" 2 vsrc v64)"
assert_eq "R64.IMPORT rejects decreasing high words" "$BAD" "$(run EVAL "$B64; return redis.pcall('R64.IMPORT', KEYS[2], two .. hi(3) .. s .. hi(1) .. s)" 2 vsrc v64)"
assert_eq "R64.IMPORT rejects trailing bytes" "$BAD" "$(run EVAL "$B64; return redis.pcall('R64.IMPORT', KEYS[2], two .. hi(1) .. s .. hi(3) .. s .. 'x')" 2 vsrc v64)"
assert_eq "  ... nothing stored" "0" "$(run EXISTS v64)"
assert_eq "R64.IMPORT accepts increasing high words" "6" "$(run EVAL "$B64; return redis.call('R64.IMPORT', KEYS[2], two .. hi(1) .. s .. hi(3) .. s)" 2 vsrc v64)"
assert_eq "  ... under both high words" "$(printf '4294967297\n12884901889')" "$(run R64.RANGEINTARRAY v64 0 0; run R64.RANGEINTARRAY v64 3 3)"

echo "=== NO-OP WRITES ==="
# Writes that change nothing must not replicate, hit the AOF or count toward
# RDB save points. replicate_verbatim drives all three and increments the
# dirty counter, so rdb_changes_since_last_save observes them on one server.
run FLUSHALL > /dev/null
dirty() { run INFO persistence | grep rdb_changes_since_last_save | cut -d: -f2 | tr -d '\r'; }
run R.SETINTARRAY nop 5 6 7 > /dev/null
run R64.SETINTARRAY nop64 5 5000000000 > /dev/null
run R.SETBIT nopempty 1 0 > /dev/null
before=$(dirty)
assert_eq "no-op SETBIT 1 on a set bit replies 1" "1" "$(run R.SETBIT nop 5 1)"
assert_eq "no-op SETBIT 0 on a clear bit replies 0" "0" "$(run R.SETBIT nop 9 0)"
assert_eq "no-op APPENDINTARRAY replies OK" "OK" "$(run R.APPENDINTARRAY nop 5 6)"
assert_eq "no-op DELETEINTARRAY replies OK" "OK" "$(run R.DELETEINTARRAY nop 99)"
assert_eq "no-op CLEARBITS COUNT replies 0" "0" "$(run R.CLEARBITS nop 99 COUNT)"
assert_eq "no-op SETRANGE replies OK" "OK" "$(run R.SETRANGE nop 5 8)"
assert_eq "no-op CLEAR on an empty key replies 0" "0" "$(run R.CLEAR nopempty)"
assert_eq "no-op R64.SETBIT replies 1" "1" "$(run R64.SETBIT nop64 5000000000 1)"
assert_eq "no-op R64.SETRANGE replies OK" "OK" "$(run R64.SETRANGE nop64 5 6)"
run EVAL "return redis.call('R.IMPORT', KEYS[1], redis.call('R.EXPORT', KEYS[1]))" 1 nop > /dev/null
run R.EXPORT nop > /dev/null
run R.GETINTARRAY nop > /dev/null
assert_eq "no-op writes and reads leave the dirty counter alone" "$before" "$(dirty)"
run R.SETBIT nop 9 1 > /dev/null
assert_eq "a real SETBIT counts one change" "$((before + 1))" "$(dirty)"
run R.SETRANGE nop 7 10 > /dev/null
assert_eq "a partly new SETRANGE counts" "$((before + 2))" "$(dirty)"
assert_eq "SETBIT 0 on a missing key still creates it (upstream)" "0" "$(run R.SETBIT ghost 3 0)"
assert_eq "created key exists" "1" "$(run EXISTS ghost)"
assert_eq "key creation counts as a change" "$((before + 3))" "$(dirty)"

echo "=== REPLICATION PROPAGATION ==="
run FLUSHALL > /dev/null
PRIMARY_CID=$(docker compose ps -q valkey)
NET=$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}' "$PRIMARY_CID")
IMG=$(docker inspect -f '{{.Config.Image}}' "$PRIMARY_CID")
docker rm -f vr-test-replica > /dev/null 2>&1 || true
docker run -d --rm --name vr-test-replica --network "$NET" "$IMG" \
  valkey-server --loadmodule /usr/lib/valkey/modules/libvalkey_roaring.so \
  --replicaof valkey 6379 > /dev/null
RCLI="docker exec vr-test-replica valkey-cli"
for _ in $(seq 1 30); do
  if $RCLI INFO replication 2>/dev/null | grep -q "master_link_status:up"; then break; fi
  sleep 1
done
run R.SETBIT repl32 42 1 > /dev/null
run R64.SETINTARRAY repl64 5 6 7 > /dev/null
run R.BITOP NOT repldest repl32 > /dev/null
run R.SETINTARRAY repldel 1 2 3 > /dev/null
run R.DELETEINTARRAY repldel 2 > /dev/null
sleep 2
assert_eq "replica got R.SETBIT" "1" "$($RCLI R.GETBIT repl32 42)"
assert_eq "replica got R64.SETINTARRAY" "3" "$($RCLI R64.BITCOUNT repl64)"
assert_eq "replica got BITOP dest" "42" "$($RCLI R.BITCOUNT repldest)"
result=$($RCLI R.GETINTARRAY repldel)
expected=$(printf "1\n3")
assert_eq "replica got DELETEINTARRAY effect" "$expected" "$result"
docker rm -f vr-test-replica > /dev/null 2>&1 || true

# -------------------------------------------------------
echo "=== AOF PERSISTENCE ==="
docker rm -f vr-test-aof > /dev/null 2>&1 || true
docker run -d --name vr-test-aof --network "$NET" "$IMG" \
  valkey-server --loadmodule /usr/lib/valkey/modules/libvalkey_roaring.so \
  --appendonly yes > /dev/null
ACLI="docker exec vr-test-aof valkey-cli"
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
$ACLI R.SETBIT aofk 7 1 > /dev/null
$ACLI R64.SETBIT aofk64 5000000000 1 > /dev/null
sleep 1
docker restart vr-test-aof > /dev/null
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
assert_eq "AOF replay restores module write" "1" "$($ACLI R.GETBIT aofk 7)"
assert_eq "AOF replay restores R64 write" "1" "$($ACLI R64.GETBIT aofk64 5000000000)"
$ACLI BGREWRITEAOF > /dev/null
sleep 2
docker restart vr-test-aof > /dev/null
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
assert_eq "AOF rewrite (RDB preamble) preserves data" "1" "$($ACLI R.GETBIT aofk 7)"
docker rm -f vr-test-aof > /dev/null 2>&1 || true

# Without the RDB preamble a rewrite calls each type's aof_rewrite callback,
# which re-emits every key as one R.IMPORT / R64.IMPORT of its blob.
docker run -d --name vr-test-aof --network "$NET" "$IMG" \
  valkey-server --loadmodule /usr/lib/valkey/modules/libvalkey_roaring.so \
  --appendonly yes --aof-use-rdb-preamble no > /dev/null
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
$ACLI R.SETINTARRAY legacy32 1 2 70000 > /dev/null
$ACLI R.SETRANGE legacy32 100 200 > /dev/null
$ACLI R64.SETINTARRAY legacy64 3 5000000000 18446744073709551615 > /dev/null
$ACLI R.SETBIT legacyempty 4 0 > /dev/null
$ACLI BGREWRITEAOF > /dev/null
for _ in $(seq 1 30); do
  info=$($ACLI INFO persistence)
  [[ "$info" == *"aof_rewrite_in_progress:0"* && "$info" == *"aof_rewrite_scheduled:0"* ]] && break
  sleep 1
done
assert_contains "AOF rewrite without preamble succeeds" "aof_last_bgrewrite_status:ok" "$($ACLI INFO persistence)"
docker restart vr-test-aof > /dev/null
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
assert_eq "legacy AOF restores R cardinality" "103" "$($ACLI R.BITCOUNT legacy32)"
assert_eq "legacy AOF restores R values" "1" "$($ACLI R.GETBIT legacy32 70000)"
assert_eq "legacy AOF restores R64 max value" "18446744073709551615" "$($ACLI R64.MAX legacy64)"
assert_eq "legacy AOF restores R64 cardinality" "3" "$($ACLI R64.BITCOUNT legacy64)"
assert_eq "legacy AOF restores an empty key" "1" "$($ACLI EXISTS legacyempty)"
docker rm -f vr-test-aof > /dev/null 2>&1 || true

# Commands written by valkey-roaring 1.1.1 replay from its AOF even where
# a client now gets an error: 1.1.1 accepted IMPORT blobs with trailing
# bytes or repeated/decreasing 64-bit high words, "+5"/"007" values, "01"
# bits and lowercase BITOP operations, and logged them verbatim. They are
# appended here to an AOF by hand, as 1.1.1 would have written them.
docker run -d --name vr-test-aof --network "$NET" "$IMG" \
  valkey-server --loadmodule /usr/lib/valkey/modules/libvalkey_roaring.so \
  --appendonly yes > /dev/null
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
$ACLI SET aofmarker 1 > /dev/null
sleep 1.5
INCR=$($ACLI CONFIG GET appenddirname | tail -1)/$(docker exec vr-test-aof sh -c 'ls "$(valkey-cli CONFIG GET appenddirname | tail -1)"' | grep incr | tail -1)
docker stop vr-test-aof > /dev/null
AOFTMP=$(mktemp -d)
docker cp "vr-test-aof:/data/$INCR" "$AOFTMP/incr.aof"
B12='\x3a\x30\x00\x00\x01\x00\x00\x00\x00\x00\x01\x00\x10\x00\x00\x00\x01\x00\x02\x00'  # R blob {1,2}
B7='\x3a\x30\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x10\x00\x00\x00\x07\x00'          # R blob {7}
{
  printf '*3\r\n$8\r\nR.IMPORT\r\n$5\r\nlgtrl\r\n$24\r\n'"$B12"'JUNK\r\n'
  printf '*3\r\n$10\r\nR64.IMPORT\r\n$5\r\nlgdup\r\n$54\r\n\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00'"$B12"'\x00\x00\x00\x00'"$B7"'\r\n'
  printf '*3\r\n$10\r\nR64.IMPORT\r\n$5\r\nlgdec\r\n$54\r\n\x02\x00\x00\x00\x00\x00\x00\x00\x05\x00\x00\x00'"$B12"'\x02\x00\x00\x00'"$B7"'\r\n'
  printf '*4\r\n$8\r\nR.SETBIT\r\n$6\r\nlgplus\r\n$2\r\n+5\r\n$2\r\n01\r\n'
  printf '*5\r\n$16\r\nR.APPENDINTARRAY\r\n$6\r\nlgplus\r\n$3\r\n007\r\n$2\r\n+9\r\n$2\r\n10\r\n'
  printf '*5\r\n$7\r\nR.BITOP\r\n$2\r\nor\r\n$4\r\nlgor\r\n$5\r\nlgtrl\r\n$6\r\nlgplus\r\n'
} >> "$AOFTMP/incr.aof"
docker cp "$AOFTMP/incr.aof" "vr-test-aof:/data/$INCR"
rm -rf "$AOFTMP"
docker start vr-test-aof > /dev/null
for _ in $(seq 1 30); do [ "$($ACLI PING 2>/dev/null)" = "PONG" ] && break; sleep 1; done
assert_eq "1.1.1 AOF: IMPORT with trailing bytes replays" "$(printf '1\n2')" "$($ACLI R.GETINTARRAY lgtrl)"
assert_eq "1.1.1 AOF: a repeated high word keeps the last entry, as 1.1.1 did" "7" "$($ACLI R64.GETINTARRAY lgdup)"
assert_eq "1.1.1 AOF: decreasing high words replay" "$(printf '8589934599\n21474836481\n21474836482')" "$($ACLI R64.GETINTARRAY lgdec)"
assert_eq "1.1.1 AOF: +5 / 01 / 007 / +9 replay" "$(printf '5\n7\n9\n10')" "$($ACLI R.GETINTARRAY lgplus)"
assert_eq "1.1.1 AOF: a lowercase BITOP replays" "$(printf '1\n2\n5\n7\n9\n10')" "$($ACLI R.GETINTARRAY lgor)"
assert_eq "1.1.1 AOF: nothing failed on replay" "0" "$(docker logs vr-test-aof 2>&1 | grep -c CRITICAL)"
assert_eq "clients still get the strict grammar" "ERR invalid offset: must be an unsigned 32 bit integer" "$($ACLI R.SETBIT lgplus +5 1)"
assert_eq "clients still get strict IMPORT" "ERR bad binary data for roaring" "$($ACLI EVAL "return redis.pcall('R.IMPORT', KEYS[1], redis.call('R.EXPORT', KEYS[1]) .. 'JUNK')" 1 lgtrl)"
docker rm -f vr-test-aof > /dev/null 2>&1 || true

# -------------------------------------------------------
echo "=== RDB PERSISTENCE ==="
run FLUSHALL > /dev/null
run R.SETINTARRAY persist32 10 20 30 > /dev/null
run R64.SETINTARRAY persist64 1 5000000000 > /dev/null
run BGSAVE > /dev/null
sleep 1

# Restart Valkey
docker compose restart valkey > /dev/null 2>&1
sleep 2

result=$(run R.GETINTARRAY persist32)
expected=$(printf "10\n20\n30")
assert_eq "RDB persist 32-bit" "$expected" "$result"

result=$(run R64.GETINTARRAY persist64)
expected=$(printf "1\n5000000000")
assert_eq "RDB persist 64-bit" "$expected" "$result"

# -------------------------------------------------------
echo ""
echo "========================================"
echo "  PASSED: ${PASS}"
echo "  FAILED: ${FAIL}"
echo "========================================"
if [[ $FAIL -gt 0 ]]; then
  echo -e "\nFailures:${ERRORS}"
  exit 1
fi
echo "  ALL TESTS PASSED"
