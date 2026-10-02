# Persistence & Replication

## RDB

Bitmaps serialize into RDB snapshots using the CRoaring portable format.
`SAVE`, `BGSAVE`, and server restarts round-trip module keys exactly; the
registered type names are `vrroaring` (32-bit) and `vroarng64` (64-bit).

## AOF

Supported in both modes. Every write command that changes a key propagates
verbatim into the AOF stream. With Valkey's default
`aof-use-rdb-preamble yes`, rewrites embed the RDB serialization as the
base; with `aof-use-rdb-preamble no`, the type's rewrite callback re-emits
each key as one `R.IMPORT` / `R64.IMPORT` of its portable blob.

## Replication

Every write command replicates verbatim to replicas. Replica-side state is
byte-identical — validated by attaching a live replica under concurrent
write load and comparing exported blobs for every key.

## Upgrading from 1.1.1

RDB files and DUMP payloads written by 1.1.1 load unchanged. Its AOF and
its replication stream carry commands verbatim, and 1.1.1 accepted some
input that clients now get an error for: `+5` / `007` values, `01` bits,
lowercase BITOP operations, and IMPORT blobs with trailing bytes or with
64-bit high words that repeat or decrease. Commands replayed from the AOF
or received from a primary are therefore still read with 1.1.1's rules,
with 1.1.1's results (a repeated high word keeps the last entry), so a
replica of a 1.1.1 primary and a reloaded 1.1.1 AOF hold the same sets
the 1.1.1 server held.

Recommended anyway: run `BGREWRITEAOF` on the 1.1.1 server before
upgrading. The rewrite folds the command history into the RDB preamble,
so nothing of 1.1.1's input rules is left to replay. (It also covers the
one case the rules above cannot: 1.1.1 read `SETBITARRAY` arguments as
lossy UTF-8, so a non-UTF-8 argument replays as the raw bytes it was.)

## Generic key machinery

Module keys participate in the keyspace like native types:

- `TYPE`, `EXISTS`, `DEL`, `UNLINK`, `RENAME`, `SCAN` (including
  `TYPE vrroaring` filters)
- `COPY` performs a deep copy of the bitmap
- `DUMP` / `RESTORE` round-trip module keys (with `REPLACE` support)
- `EXPIRE` / `PERSIST` / `TTL` behave normally
- `MEMORY USAGE` estimates the bitmap's in-memory footprint (containers,
  their spare capacity and allocator rounding), typically within 15% of what
  the allocator reports; the serialized `R.EXPORT` size is usually smaller

Module write commands do not emit keyspace notifications; server-generated
events such as `expired` fire normally for module keys.

## Cluster mode

`R.BITOP` reports its key positions through the module getkeys API, so
`COMMAND GETKEYS`, ACL checks, and cluster slot validation are accurate even
for the trailing non-key `last` argument of `R.BITOP NOT`. Hash-tagged
same-slot operations work; cross-slot combinations are rejected with the
standard `CROSSSLOT` error.
