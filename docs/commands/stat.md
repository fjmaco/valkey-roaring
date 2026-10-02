# R.STAT

Returns statistics about a bitmap: type, cardinality, min/max, and the full
array/bitset/run container breakdown. Shared between both widths — it
auto-detects whether the key holds a 32-bit or 64-bit bitmap.

| | |
|---|---|
| **Syntax** | `R.STAT key [TEXT\|JSON]` |
| **Time complexity** | O(containers) |

## Arguments

- **key** — the bitmap key (either width)
- **format** — `JSON` for JSON; anything else (or nothing) for plain text. The token is case-sensitive, as upstream's.

## Reply

A verbatim string (`txt`) under RESP3, a bulk string under RESP2, laid out
byte for byte as redis-roaring lays it out; null for a missing key.

The counters use CRoaring's units: array container bytes are 2 per value,
bitset container bytes 8,192 per container, run container bytes 2 plus 4
per run for each container. An empty bitmap reports `max value: 0` and
`min value` the width's maximum. As in redis-roaring, the 32-bit counters
are 32-bit (an `R.SETFULL` key reports `run container values: 0`).

## Notes

- The per-encoding container breakdown (the array, bitset and run lines)
  reflects this module's container encodings, which can differ from
  CRoaring's after range inserts and set operations: CRoaring stores a
  2-value `SETRANGE` as a run where roaring-rs stores an array, an `XOR`
  result can come out the other way round, and `BITOP NOT` can leave
  runs where CRoaring holds a bitset. Every other field (type, cardinality,
  number of containers, max, min) matches redis-roaring, as do the layout
  and the units, and the breakdown agrees too after
  [OPTIMIZE](/commands/optimize).

## Example

```bash
$ valkey-cli R.SETINTARRAY k 1 2 3
OK
$ valkey-cli --raw R.STAT k
type: bitmap
cardinality: 3
number of containers: 1
max value: 3
min value: 1
number of array containers: 1
	array container values: 3
	array container bytes: 6
bitset  containers: 0
	bitset  container values: 0
	bitset  container bytes: 0
run containers: 0
	run container values: 0
	run container bytes: 0
$ valkey-cli --raw R.STAT k JSON
{"type":"bitmap","cardinality":"3","number_of_containers":"1",...}
```
