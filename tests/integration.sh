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
expected=$(printf "0\n0\n0")
assert_eq "GETBITS nonexistent key" "$expected" "$result"

# -------------------------------------------------------
echo "--- CLEARBITS ---"
run R.SETINTARRAY k3 1 2 3 4 5 > /dev/null
assert_eq "CLEARBITS clears existing bits" "3" "$(run R.CLEARBITS k3 1 3 5 99)"
result=$(run R.GETINTARRAY k3)
expected=$(printf "2\n4")
assert_eq "CLEARBITS remaining values" "$expected" "$result"
assert_eq "CLEARBITS on nonexistent key" "0" "$(run R.CLEARBITS nonexist 1 2)"

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
run R.SETINTARRAY k8 5 10 15 20 25 30 > /dev/null
result=$(run R.RANGEINTARRAY k8 10 25)
expected=$(printf "10\n15\n20\n25")
assert_eq "RANGEINTARRAY returns range" "$expected" "$result"
result=$(run R.RANGEINTARRAY k8 100 200)
assert_eq "RANGEINTARRAY empty range" "" "$result"
result=$(run R.RANGEINTARRAY nonexist 0 100)
assert_eq "RANGEINTARRAY nonexistent key" "" "$result"

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
assert_eq "SETRANGE cardinality" "6" "$(run R.BITCOUNT k10)"
assert_eq "SETRANGE min" "5" "$(run R.MIN k10)"
assert_eq "SETRANGE max" "10" "$(run R.MAX k10)"
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
assert_eq "CONTAINS NONE (overlap)" "1" "$(run R.CONTAINS ca cb)"
assert_eq "CONTAINS NONE (no overlap)" "0" "$(run R.CONTAINS ca cd)"
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
echo "=== UPSTREAM v1.7.3/v1.7.4 PARITY ==="
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
echo "--- BITOP NOT on empty/missing source (v1.7.4) ---"
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
echo "--- BITOP getkeys (cluster routing, v1.7.3) ---"
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
echo "--- BITOP invalid operation error reply (v1.7.4) ---"
assert_contains "BITOP invalid op is an error" "syntax error" "$(run R.BITOP FOO d s1 s2)"
assert_contains "R64 BITOP invalid op is an error" "syntax error" "$(run R64.BITOP BAR d s1 s2)"

# -------------------------------------------------------
echo "--- CLEARBITS duplicate offsets (v1.7.4) ---"
run R.SETINTARRAY dupck 5 7 > /dev/null
assert_eq "CLEARBITS duplicate offsets count once" "1" "$(run R.CLEARBITS dupck 5 5 5)"
result=$(run R.GETINTARRAY dupck)
assert_eq "CLEARBITS duplicates leave other bits" "7" "$result"
run R64.SETINTARRAY dupck64 5 7 > /dev/null
assert_eq "R64 CLEARBITS duplicate offsets count once" "1" "$(run R64.CLEARBITS dupck64 5 5 5)"

# -------------------------------------------------------
echo "--- DELETEINTARRAY duplicate deletes of last value (v1.7.4) ---"
run R64.SETINTARRAY dupdel64 100 > /dev/null
assert_eq "R64 DELETEINTARRAY duplicate deletes OK" "OK" "$(run R64.DELETEINTARRAY dupdel64 100 100 100)"
assert_eq "R64 DELETEINTARRAY duplicate deletes result" "0" "$(run R64.BITCOUNT dupdel64)"
run R.SETINTARRAY dupdel 100 > /dev/null
assert_eq "DELETEINTARRAY duplicate deletes OK" "OK" "$(run R.DELETEINTARRAY dupdel 100 100 100)"
assert_eq "DELETEINTARRAY duplicate deletes result" "0" "$(run R.BITCOUNT dupdel)"

# -------------------------------------------------------
echo "--- BITPOS edge cases (v1.7.4) ---"
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
echo "--- R64.OPTIMIZE (roaring 0.11.4+) ---"
run R64.SETRANGE optr64 0 100000 > /dev/null
assert_eq "R64 OPTIMIZE returns OK" "OK" "$(run R64.OPTIMIZE optr64)"
assert_eq "R64 OPTIMIZE preserves data" "100001" "$(run R64.BITCOUNT optr64)"

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
    "$prefix.BITOP AND wtdest plainstr"
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
assert_contains "SETBIT offset out of 32-bit range" "out of range" "$(run R.SETBIT badkey 4294967296 1)"
assert_contains "CONTAINS invalid mode" "invalid mode" "$(run R.CONTAINS wtsrc wtsrc BOGUS)"
assert_contains "SETRANGE inverted range" "must be >= start" "$(run R.SETRANGE rangekey 5 2)"

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
