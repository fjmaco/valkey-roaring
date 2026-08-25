# Getting Started

## Run from Docker Hub

The fastest path — a prebuilt image with Valkey 8.1 and the module loaded:

```bash
docker run -d -p 6379:6379 fjmaco/valkey-roaring
```

## Build with Docker Compose

From a clone of the [repository](https://github.com/fjmaco/valkey-roaring):

```bash
docker compose up -d          # builds the module from source, starts Valkey
docker compose exec valkey valkey-cli
```

## Build from source

Requirements: Rust 1.90+, libclang.

```bash
cargo build --release
# Output: target/release/libvalkey_roaring.so

valkey-server --loadmodule ./target/release/libvalkey_roaring.so
```

Or in `valkey.conf`:

```
loadmodule /path/to/libvalkey_roaring.so
```

## Using Redis instead of Valkey

The module initializes through the RedisModule API, which both servers expose,
so the same `.so` loads into Redis 7.4+ unchanged — commands, replication, and
RDB persistence all work identically:

```bash
redis-server --loadmodule ./target/release/libvalkey_roaring.so
```

## First commands

```bash
127.0.0.1:6379> R.SETBIT users:active 42 1
(integer) 0
127.0.0.1:6379> R.SETBIT users:active 100 1
(integer) 0
127.0.0.1:6379> R.BITCOUNT users:active
(integer) 2

127.0.0.1:6379> R.SETINTARRAY signup:day1 1 2 3 4 5
OK
127.0.0.1:6379> R.SETINTARRAY purchase:day1 3 4 5 6 7
OK
127.0.0.1:6379> R.BITOP AND converted signup:day1 purchase:day1
(integer) 3
127.0.0.1:6379> R.GETINTARRAY converted
1) (integer) 3
2) (integer) 4
3) (integer) 5
```

64-bit values use the `R64.` prefix:

```bash
127.0.0.1:6379> R64.SETBIT big 5000000000 1
(integer) 0
127.0.0.1:6379> R64.MAX big
(integer) 5000000000
```

Continue with the [command reference](/commands/) or jump straight to
[exporting bitmaps to other services](/guide/export-import).
