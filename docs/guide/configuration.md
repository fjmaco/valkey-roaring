# Configuration

The module has two configuration parameters. Both are standard module
configs: set them when the module loads or change them at runtime with
`CONFIG SET`.

| Parameter | Default | Allowed values | Bounds |
|-----------|---------|----------------|--------|
| `valkey-roaring.max-reply-elements` | 100000000 | 1 to 4294967296 | the elements one reply may list |
| `valkey-roaring.max-write-values` | 274877906944 (2³⁸) | 1 to 274877906944 | the contiguous values one write may build |

## Setting them

In `valkey.conf`, after the `loadmodule` line:

```
loadmodule /path/to/libvalkey_roaring.so
valkey-roaring.max-reply-elements 10000000
valkey-roaring.max-write-values 4294967296
```

On the command line:

```bash
valkey-server --loadmodule ./libvalkey_roaring.so \
  --valkey-roaring.max-reply-elements 10000000
```

When loading at runtime:

```
MODULE LOADEX /path/to/libvalkey_roaring.so CONFIG valkey-roaring.max-write-values 4294967296
```

At runtime:

```
127.0.0.1:6379> CONFIG SET valkey-roaring.max-reply-elements 10000000
OK
127.0.0.1:6379> CONFIG GET valkey-roaring.max-reply-elements
1) "valkey-roaring.max-reply-elements"
2) "10000000"
```

A value outside the allowed range is refused by `CONFIG SET`. Given at
startup, it stops the server from starting, as an invalid server setting
does.

## max-reply-elements

The most elements `GETINTARRAY`, `RANGEINTARRAY` and `GETBITARRAY` may
reply with. Beyond it they reply
`Roaring: range too large: maximum <value> elements` before writing
anything:

- `GETINTARRAY` of a bitmap with more values than the limit;
- `RANGEINTARRAY` with a window of more positions than the limit, or the
  full-width request (`0` to the width's maximum) on a bitmap with more
  values than the limit;
- `GETBITARRAY` of a bitmap whose maximum is at or above the limit (the
  reply is maximum + 1 characters).

Each listed value costs about 10 to 20 bytes of client output buffer (one
RESP integer), so the default already allows replies of one to two
gigabytes. Raise it only as far as the server's memory allows. A large bitmap can always be paged with
`RANGEINTARRAY` or transferred with `EXPORT` instead.

## max-write-values

The most values one write may build as a contiguous range: `SETRANGE`,
`SETFULL`, and the `[0, max(last, source max)]` universe of `BITOP NOT`.
Past it these reply `Roaring: range too large: maximum <value> elements`
and change nothing.

The default, 2³⁸, is also the highest allowed value: 4,194,304 full
containers, about 200 MB stored as runs and about 0.1 s of work. The whole
32-bit space (`R.SETFULL`, 2³² values, about 3 MB) fits within it. Lower it
to bound what one command can allocate; `R.SETFULL` needs at least
4294967296.

The limit applies to commands from clients. A write replayed from the AOF
or received from a primary was accepted where it first ran, so it is
applied even when this server's limit is lower, up to 2³⁸: a replica or a
restart never loses data because of this setting.

## Memory: maxmemory

`SETRANGE`, `SETFULL` and `BITOP NOT` can build far more data than their
arguments suggest. When `maxmemory` is set, a range write whose estimated
result would push used memory past `maxmemory` is refused before anything
is allocated, with the server's standard reply:

```
OOM command not allowed when used memory > 'maxmemory'.
```

- The estimate counts the containers the range spans, at 50 to 90 bytes
  each (a container record, its one-run payload and vector growth). Writes
  estimated below 1 MB are not checked: they add no more than an ordinary
  small write does.
- Under an eviction policy (`maxmemory-policy` other than `noeviction`) the
  server makes room after the write by evicting keys, as it does for any
  write, so only a write larger than `maxmemory` itself is refused.
- Without `maxmemory` nothing is checked.
- Writes replayed from the AOF or received from a primary are never
  refused, and nothing is checked on a replica (replicas ignore
  `maxmemory` by default).
