# Persistence & Replication

## RDB

Bitmaps serialize into RDB snapshots using the CRoaring portable format.
`SAVE`, `BGSAVE`, and server restarts round-trip module keys exactly; the
registered type names are `vrroaring` (32-bit) and `vroarng64` (64-bit).

## AOF

Fully supported with Valkey's default configuration
(`aof-use-rdb-preamble yes`): every write command propagates verbatim into
the AOF stream, and rewrites embed the RDB serialization as the base. The
legacy non-preamble AOF mode is the one unsupported configuration — the
Valkey Rust SDK does not expose the per-type `EmitAOF` rewrite callback it
would need.

## Replication

Every write command replicates verbatim to replicas. Replica-side state is
byte-identical — validated by attaching a live replica under concurrent
write load and comparing exported blobs for every key.

## Generic key machinery

Module keys participate in the keyspace like native types:

- `TYPE`, `EXISTS`, `DEL`, `UNLINK`, `RENAME`, `SCAN` (including
  `TYPE vrroaring` filters)
- `COPY` performs a deep copy of the bitmap
- `DUMP` / `RESTORE` round-trip module keys (with `REPLACE` support)
- `EXPIRE` / `PERSIST` / `TTL` behave normally
- `MEMORY USAGE` reports the bitmap's serialized footprint

Module write commands do not emit keyspace notifications (matching
redis-roaring); server-generated events such as `expired` fire normally for
module keys.

## Cluster mode

`R.BITOP` reports its key positions through the module getkeys API, so
`COMMAND GETKEYS`, ACL checks, and cluster slot validation are accurate even
for the trailing non-key `last` argument of `R.BITOP NOT`. Hash-tagged
same-slot operations work; cross-slot combinations are rejected with the
standard `CROSSSLOT` error.
