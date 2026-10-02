# Command Reference

Every command exists in a 32-bit (`R.*`, values 0 … 2³²−1) and a 64-bit
(`R64.*`, values 0 … 2⁶⁴−1) form with identical semantics — both are
generated from one generic implementation. Each page below documents the
family once. `R.STAT` is shared and auto-detects the key's width.

**Total: 51 commands** (25 `R.*` + 25 `R64.*` + `R.STAT`).

| Group | Commands |
|-------|----------|
| Bit access | [SETBIT](/commands/setbit), [GETBIT](/commands/getbit), [GETBITS](/commands/getbits), [CLEARBITS](/commands/clearbits), [CLEAR](/commands/clear) |
| Integer arrays | [SETINTARRAY](/commands/setintarray), [GETINTARRAY](/commands/getintarray), [APPENDINTARRAY](/commands/appendintarray), [DELETEINTARRAY](/commands/deleteintarray), [RANGEINTARRAY](/commands/rangeintarray) |
| Bit strings | [SETBITARRAY](/commands/setbitarray), [GETBITARRAY](/commands/getbitarray) |
| Ranges | [SETRANGE](/commands/setrange), [SETFULL](/commands/setfull) |
| Aggregation | [BITCOUNT](/commands/bitcount), [BITPOS](/commands/bitpos), [MIN](/commands/min), [MAX](/commands/max) |
| Set algebra | [BITOP](/commands/bitop), [CONTAINS](/commands/contains), [JACCARD](/commands/jaccard), [DIFF](/commands/diff) |
| Interchange | [EXPORT](/commands/export), [IMPORT](/commands/import) |
| Maintenance | [OPTIMIZE](/commands/optimize), [STAT](/commands/stat) |

Replies for 64-bit values above 2⁶³−1 arrive as decimal strings (RESP
integers are signed 64-bit); everything else is a plain integer reply.

## Arguments and errors

Arguments are read exactly as redis-roaring reads them, and errors carry
its wording:

- **32-bit values** are `0` or a non-zero digit followed by digits, at most
  4294967295: no sign, no leading zeros, no spaces. Anything else replies
  `ERR invalid <name>: must be an unsigned 32 bit integer`.
- **64-bit values** are digits with an optional leading `+`; leading zeros
  are allowed. Anything else replies
  `ERR invalid <name>: must be an unsigned 64 bit integer`.
- **Bits** (SETBIT's value, BITPOS's bit) are exactly `0` or `1`, else
  `ERR invalid <name>: must be either 0 or 1`.
- **Tokens** are case-sensitive: BITOP operations, CONTAINS modes,
  CLEARBITS's `COUNT` and STAT's `JSON` match only in upper case.
- The key is looked at before the arguments, so a key of another type
  replies `WRONGTYPE` even when an argument is also invalid.
- Commands replayed from the AOF or received from a primary also accept
  what valkey-roaring 1.1.1 accepted (`+5`, `007`, `01`, lowercase BITOP
  operations), so a 1.1.1 AOF or primary replays with its results; see
  [Upgrading from 1.1.1](/guide/persistence-and-replication#upgrading-from-1-1-1).

## Limits

- Replies that list values (GETINTARRAY, RANGEINTARRAY, GETBITARRAY) are
  capped at 100,000,000 elements and refused with
  `Roaring: range too large: maximum 100000000 elements` beyond it.
- A single write may build at most 2³⁸ (274,877,906,944) contiguous values
  — 4,194,304 full containers, about 200 MB stored as runs, in roughly
  0.1 s. R64.SETFULL, an R64.SETRANGE wider than that, and an R64.BITOP NOT
  whose universe `[0, max(last, source max)]` exceeds it are refused up
  front with `Roaring: range too large: maximum 274877906944 elements`.
  The whole 32-bit space (R.SETFULL, R.BITOP NOT up to 4294967295, a full
  2³² sub-bitmap under R64) fits well within it. The bound is per call:
  one write may take memory up to about 200 MB past `maxmemory`, after
  which the server refuses further writes (the commands are `deny-oom`)
  until memory is freed.
- GETINTARRAY refuses a bitmap of more than 100,000,000 values by design;
  page through it with RANGEINTARRAY (at most 100,000,000 positions per
  call) or transfer it with EXPORT.

## Differences from redis-roaring

Replies match redis-roaring's byte for byte, except in these cases, where
upstream crashes, hangs, overflows or gives a wrong answer:

| Command | redis-roaring | valkey-roaring |
|---------|---------------|----------------|
| `R.SETBIT newkey 7 0` | sets bit 7 | creates an empty key; bit 7 stays clear |
| `R.BITOP NOT dest src 4294967295` | `last + 1` overflows: a copy of the source | the complement over the whole 32-bit space |
| `R64.BITOP NOT` over a universe past 2³⁸ | allocates until killed, or (`last` = 2⁶⁴−1) overflows to a copy | `Roaring: range too large: maximum 274877906944 elements` |
| `R64.SETFULL`, `R64.SETRANGE` past 2³⁸ values | allocates until killed | the same error |
| variadic `R.BITOP` with a wrong-type source | two `WRONGTYPE` replies for one command | one `WRONGTYPE` reply |
| `R64.RANGEINTARRAY key 0 18446744073709551615` | `ERR out of memory` | the whole bitmap, as `R.RANGEINTARRAY key 0 4294967295` replies |
| `GETINTARRAY` past 100,000,000 values | streams every value (2³² for `R.SETFULL`) | `Roaring: range too large: maximum 100000000 elements` |
| full-width `RANGEINTARRAY` past 100,000,000 values | tries to list every value | the same error |
| `GETBITARRAY` with a maximum at or past 100,000,000 | builds the string; crashes on `R.SETFULL` | the same error |
| `R.STAT`'s per-encoding container breakdown after range inserts and set operations | CRoaring's encodings (a 2-value `SETRANGE` is a run) | this module's (an array); every other field matches, and so does the breakdown after `OPTIMIZE` |

`R.EXPORT` / `R.IMPORT` are valkey-roaring additions.
