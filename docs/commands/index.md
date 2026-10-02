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

Arguments are parsed strictly, and the error replies below are part of the
reply contract:

- **32-bit values** are `0` or a non-zero digit followed by digits, at most
  4294967295: no sign, no leading zeros, no spaces. Anything else replies
  `ERR invalid <name>: must be an unsigned 32 bit integer`.
- **64-bit values** are digits with an optional leading `+`; leading zeros
  are allowed. Anything else replies
  `ERR invalid <name>: must be an unsigned 64 bit integer`.
- **Bits** (SETBIT's value, BITPOS's bit) are exactly `0` or `1`, else
  `ERR invalid <name>: must be either 0 or 1`.
- **Tokens** are case-sensitive: BITOP operations, CONTAINS modes,
  CLEARBITS's `COUNT`, STAT's `JSON` and EXPORT/IMPORT's `BASE64` match
  only in upper case.
- The key is looked at before the arguments, so a key of another type
  replies `WRONGTYPE` even when an argument is also invalid.
- Commands replayed from the AOF or received from a primary also accept
  what valkey-roaring 1.1.1 accepted (`+5`, `007`, `01`, lowercase BITOP
  operations), so a 1.1.1 AOF or primary replays with its results; see
  [Upgrading from 1.1.1](/guide/persistence-and-replication#upgrading-from-1-1-1).

## Limits

Both size limits are [module configuration parameters](/guide/configuration);
the values below are the defaults, and a refusal always names the value in
effect.

- Replies that list values (GETINTARRAY, RANGEINTARRAY, GETBITARRAY) are
  capped at `valkey-roaring.max-reply-elements` elements, 100,000,000 by
  default, and refused with
  `Roaring: range too large: maximum 100000000 elements` beyond it.
- A single write may build at most `valkey-roaring.max-write-values`
  contiguous values: 2³⁸ (274,877,906,944) by default, which is also the
  highest it can be set to — 4,194,304 full containers, about 200 MB
  stored as runs, in roughly 0.1 s. R64.SETFULL, an R64.SETRANGE wider than
  that, and an R64.BITOP NOT whose universe `[0, max(last, source max)]`
  exceeds it are refused up front with
  `Roaring: range too large: maximum 274877906944 elements`. The whole
  32-bit space (R.SETFULL, R.BITOP NOT up to 4294967295, a full 2³²
  sub-bitmap under R64) fits well within it. Writes replayed from the AOF
  or received from a primary are applied even when this server's limit is
  lower.
- With `maxmemory` set, a range write (SETRANGE, SETFULL, BITOP NOT) whose
  estimated result would push used memory past `maxmemory` is refused up
  front with the server's standard reply,
  `OOM command not allowed when used memory > 'maxmemory'.`, and changes
  nothing. Under an eviction policy only a write larger than `maxmemory`
  itself is refused; see
  [Configuration](/guide/configuration#memory-maxmemory).
- GETINTARRAY refuses a bitmap of more values than the reply limit by
  design; page through it with RANGEINTARRAY or transfer it with EXPORT.
- The full 64-bit space cannot be stored: R64.SETFULL would need 2³² full
  32-bit sub-bitmaps (2⁴⁸ containers), so it is always refused.

## Edge cases

| Case | Reply |
|------|-------|
| `R.SETBIT newkey 7 0` | creates an empty key; bit 7 stays clear |
| `R.BITOP NOT dest src 4294967295` | the complement over the whole 32-bit space |
| `R64.BITOP NOT` over a universe past 2³⁸, `last` = 2⁶⁴−1 included | `Roaring: range too large: maximum 274877906944 elements` |
| `R64.SETFULL`, `R64.SETRANGE` past 2³⁸ values | the same error |
| a range write refused under a lowered `valkey-roaring.max-write-values` (say 1000) | `Roaring: range too large: maximum 1000 elements` |
| `SETRANGE`, `SETFULL` or `BITOP NOT` whose estimated result would push used memory past `maxmemory` | `OOM command not allowed when used memory > 'maxmemory'.` (nothing is created or changed) |
| variadic `R.BITOP` with a wrong-type source | a single `WRONGTYPE` reply |
| `R64.RANGEINTARRAY key 0 18446744073709551615` | the whole bitmap, as `R.RANGEINTARRAY key 0 4294967295` replies |
| `GETINTARRAY` past 100,000,000 values | `Roaring: range too large: maximum 100000000 elements` |
| full-width `RANGEINTARRAY` past 100,000,000 values | the same error |
| `GETBITARRAY` with a maximum at or past 100,000,000, an `R.SETFULL` key included | the same error |
| any of these under a lowered `valkey-roaring.max-reply-elements` (say 5000) | `Roaring: range too large: maximum 5000 elements` |
| `EXPORT key base64`, `IMPORT key text BASE` or any other trailing token | `ERR syntax error` |
| `IMPORT key text BASE64` with text that is not strict Base64 | `ERR bad binary data for roaring` |
| `R.STAT`'s per-encoding container breakdown after range inserts and set operations | this module's container encodings (a 2-value `SETRANGE` is stored as an array, not a run); every other field depends only on the values, and after `OPTIMIZE` so does the breakdown (see [STAT](/commands/stat)) |
