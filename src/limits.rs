//! valkey-roaring: Reply and write limits, and the memory check for range
//! writes.
//!
//! The two limits are module configuration parameters, settable at load
//! time (a config-file line, `--valkey-roaring.<name> <value>` or
//! `MODULE LOADEX ... CONFIG`) and with CONFIG SET:
//!
//! - `valkey-roaring.max-reply-elements`: most elements a reply may list
//!   (GETINTARRAY, RANGEINTARRAY, GETBITARRAY).
//! - `valkey-roaring.max-write-values`: most values one write may build as a
//!   contiguous range (SETRANGE, SETFULL, the [0, last] universe of BITOP
//!   NOT).

use crate::commands::from_aof_or_primary;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_longlong, c_void};
use std::sync::atomic::{AtomicI64, Ordering};
use valkey_module::{raw, Context, ContextFlags, Status, ValkeyError};

/// Default for max-reply-elements: one hundred million elements, about
/// 1.3 GB of client output buffer as integer replies.
pub const DEFAULT_MAX_REPLY_ELEMENTS: i64 = 100_000_000;
/// Highest max-reply-elements: every value of a 32-bit key.
pub const MAX_REPLY_ELEMENTS_CEILING: i64 = 1 << 32;

/// Highest (and default) max-write-values: 2^38 values, 4,194,304 full
/// containers, about 200 MB stored as runs and about 0.1 s of work. A full
/// 32-bit space (2^32, R.SETFULL) fits comfortably; R64.SETFULL (2^64) and
/// ranges or NOT universes reaching far into the 64-bit space would need
/// billions of containers, which an unguarded write tried to allocate until
/// the server was killed. Setting it lower is always safe; higher is not
/// allowed.
pub const MAX_WRITE_VALUES_CEILING: i64 = 1 << 38;
pub const DEFAULT_MAX_WRITE_VALUES: i64 = MAX_WRITE_VALUES_CEILING;

static MAX_REPLY_ELEMENTS: AtomicI64 = AtomicI64::new(DEFAULT_MAX_REPLY_ELEMENTS);
static MAX_WRITE_VALUES: AtomicI64 = AtomicI64::new(DEFAULT_MAX_WRITE_VALUES);

/// The current max-reply-elements.
pub fn max_reply_elements() -> u64 {
    MAX_REPLY_ELEMENTS.load(Ordering::Relaxed) as u64
}

/// The current max-write-values.
pub fn max_write_values() -> u64 {
    MAX_WRITE_VALUES.load(Ordering::Relaxed) as u64
}

/// The refusal for a reply or write past `limit`. With the default limits
/// this is the long-standing reply text, byte for byte.
pub fn range_too_large(limit: u64) -> ValkeyError {
    ValkeyError::String(format!(
        "Roaring: range too large: maximum {limit} elements"
    ))
}

/// Refuses a write that would build more than max-write-values contiguous
/// values. A write replayed from the AOF or received from a primary was
/// accepted where it was first run, under that server's setting, so only
/// the ceiling applies to it: a replica or an AOF never diverges because
/// this server's setting is lower.
pub fn check_write_values(ctx: &Context, values: u64) -> Result<(), ValkeyError> {
    let limit = max_write_values();
    if values <= limit {
        return Ok(());
    }
    let ceiling = MAX_WRITE_VALUES_CEILING as u64;
    if from_aof_or_primary(ctx) {
        return if values <= ceiling {
            Ok(())
        } else {
            Err(range_too_large(ceiling))
        };
    }
    Err(range_too_large(limit))
}

// ============================================================
// Memory check for range writes
// ============================================================

/// The server's own reply to a deny-oom command over maxmemory.
pub const ERR_OOM: &str = "OOM command not allowed when used memory > 'maxmemory'.";

/// Range writes estimated below this many bytes skip the memory check: they
/// overshoot maxmemory no more than an ordinary write of a few hundred
/// kilobytes does. 1 MiB is about 22,000 full containers (1.4 billion
/// values), already about half a millisecond of work, so the check (one
/// INFO read when maxmemory is set) costs a few percent of such a write at
/// most, and nothing on smaller ones. R.SETFULL (3 MiB) is checked.
pub const MEMORY_CHECK_FROM_BYTES: u64 = 1 << 20;

/// One roaring container record in a bitmap's container vector (see
/// bitmap32::CONTAINER_RECORD_BYTES).
const RECORD_BYTES: u64 = 40;
/// A range container's payload: one 4-byte run, allocated as 8 bytes.
const RUN_PAYLOAD_BYTES: u64 = 8;
/// Containers in a full 32-bit (sub-)bitmap.
const CONTAINERS_PER_SUB: u64 = 1 << 16;

/// Heap bytes for `containers` range containers in one container vector,
/// which a range insert grows by doubling.
fn vector_bytes(containers: u64) -> u64 {
    containers.max(4).next_power_of_two() * RECORD_BYTES + containers * RUN_PAYLOAD_BYTES
}

/// Estimated heap bytes a write of the values [first, last] stores, from
/// the containers it spans and before anything is allocated: one record and
/// one single-run payload per container, each 32-bit (sub-)bitmap in a
/// vector of its own (R64 keys hold one per 2^32 values). An upper bound
/// for range writes; containers the key already holds only lower the real
/// cost.
pub fn range_write_bytes(first: u64, last: u64) -> u64 {
    debug_assert!(first <= last);
    let key = |v: u64| (v >> 16) & 0xFFFF;
    let (first_sub, last_sub) = (first >> 32, last >> 32);
    if first_sub == last_sub {
        return vector_bytes(key(last) - key(first) + 1);
    }
    vector_bytes(CONTAINERS_PER_SUB - key(first))
        + (last_sub - first_sub - 1).saturating_mul(vector_bytes(CONTAINERS_PER_SUB))
        + vector_bytes(key(last) + 1)
}

/// Whether a write adding `bytes` fits under maxmemory, for a server whose
/// logical used memory is `ratio` (RedisModule_GetUsedMemoryRatio) times
/// `maxmemory`. Under an eviction policy the server makes room afterwards,
/// as it does for any write, so only a write larger than maxmemory itself
/// (which no eviction could make room for) does not fit.
fn fits_in_memory(ratio: f32, maxmemory: u64, bytes: u64, evicts: bool) -> bool {
    if evicts {
        return bytes <= maxmemory;
    }
    let used = (f64::from(ratio) * maxmemory as f64) as u64;
    used.saturating_add(bytes) <= maxmemory
}

/// Refuses, with the server's own OOM error, a range write whose estimated
/// result (`bytes`, see range_write_bytes) would push used memory past
/// maxmemory. deny-oom only refuses commands once memory is already over
/// the limit, so without this one write could build up to max-write-values
/// values (about 200 MB) past it.
///
/// Free below MEMORY_CHECK_FROM_BYTES and without maxmemory. Never refuses
/// a write replayed from the AOF or received from a primary (accepted where
/// it was first run), nor on a replica, which ignores maxmemory for its
/// primary's writes.
pub fn check_memory(ctx: &Context, bytes: u64) -> Result<(), ValkeyError> {
    if bytes < MEMORY_CHECK_FROM_BYTES {
        return Ok(());
    }
    // SAFETY: the API table is filled at module load; the call takes no
    // arguments and only reads the server's memory counters.
    let ratio = match unsafe { raw::RedisModule_GetUsedMemoryRatio } {
        Some(used_memory_ratio) => unsafe { used_memory_ratio() },
        None => return Ok(()),
    };
    if ratio <= 0.0 {
        return Ok(()); // no maxmemory
    }
    let flags = ctx.get_flags();
    if flags.intersects(ContextFlags::LOADING | ContextFlags::REPLICATED | ContextFlags::SLAVE) {
        return Ok(());
    }
    let maxmemory = match ctx.server_info("memory").field_unsigned("maxmemory") {
        Some(max) if max > 0 => max,
        _ => return Ok(()),
    };
    if fits_in_memory(
        ratio,
        maxmemory,
        bytes,
        flags.contains(ContextFlags::EVICTED),
    ) {
        Ok(())
    } else {
        Err(ValkeyError::Str(ERR_OOM))
    }
}

// ============================================================
// Configuration registration
// ============================================================

struct Limit {
    name: &'static CStr,
    value: &'static AtomicI64,
    default: i64,
    min: i64,
    max: i64,
}

const LIMITS: [Limit; 2] = [
    Limit {
        name: c"max-reply-elements",
        value: &MAX_REPLY_ELEMENTS,
        default: DEFAULT_MAX_REPLY_ELEMENTS,
        min: 1,
        max: MAX_REPLY_ELEMENTS_CEILING,
    },
    Limit {
        name: c"max-write-values",
        value: &MAX_WRITE_VALUES,
        default: DEFAULT_MAX_WRITE_VALUES,
        min: 1,
        max: MAX_WRITE_VALUES_CEILING,
    },
];

unsafe extern "C" fn get_limit(_name: *const c_char, privdata: *mut c_void) -> c_longlong {
    // SAFETY: privdata is the 'static AtomicI64 registered with this config.
    unsafe { &*(privdata as *const AtomicI64) }.load(Ordering::Relaxed)
}

unsafe extern "C" fn set_limit(
    _name: *const c_char,
    value: c_longlong,
    privdata: *mut c_void,
    _err: *mut *mut raw::RedisModuleString,
) -> c_int {
    // The server has already checked the value against the registered
    // bounds. SAFETY: as in get_limit.
    unsafe { &*(privdata as *const AtomicI64) }.store(value, Ordering::Relaxed);
    raw::REDISMODULE_OK as c_int
}

/// Registers the limits as module configuration parameters (called from
/// the module's OnLoad). A server without the module configuration API
/// (it arrived in Redis 7.0) still loads the module, with the defaults. A
/// value given at load time that is out of range fails the load, as an
/// invalid server setting does.
pub fn register(ctx: &Context) -> Status {
    // SAFETY: the API table is filled at module load.
    let (Some(register_numeric), Some(load_configs)) =
        (unsafe { raw::RedisModule_RegisterNumericConfig }, unsafe {
            raw::RedisModule_LoadConfigs
        })
    else {
        ctx.log_notice(
            "valkey-roaring: no module configuration API on this server; the limits keep their defaults",
        );
        return Status::Ok;
    };
    let mut registered = 0;
    for limit in &LIMITS {
        // SAFETY: called from OnLoad with its context; the name outlives the
        // call (the server copies it) and privdata is 'static.
        let status = unsafe {
            register_numeric(
                ctx.ctx,
                limit.name.as_ptr(),
                limit.default,
                raw::REDISMODULE_CONFIG_DEFAULT,
                limit.min,
                limit.max,
                Some(get_limit),
                Some(set_limit),
                None,
                std::ptr::from_ref(limit.value).cast_mut().cast(),
            )
        };
        if status == raw::REDISMODULE_OK as c_int {
            registered += 1;
        } else {
            ctx.log_warning(&format!(
                "valkey-roaring: could not register config {}; it keeps its default",
                limit.name.to_string_lossy()
            ));
        }
    }
    // Applies values given at load time (or the defaults). Required once any
    // config is registered: the server unloads a module that skips it.
    // SAFETY: as above.
    if registered > 0 && unsafe { load_configs(ctx.ctx) } != raw::REDISMODULE_OK as c_int {
        ctx.log_warning("valkey-roaring: invalid module configuration value");
        return Status::Err;
    }
    Status::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_refusals_keep_their_text() {
        // Reply texts that predate the configuration, byte for byte.
        let text = |e: ValkeyError| e.to_string();
        assert_eq!(
            text(range_too_large(max_reply_elements())),
            "Roaring: range too large: maximum 100000000 elements"
        );
        assert_eq!(
            text(range_too_large(max_write_values())),
            "Roaring: range too large: maximum 274877906944 elements"
        );
        assert_eq!(
            text(range_too_large(5_000)),
            "Roaring: range too large: maximum 5000 elements"
        );
        assert_eq!(max_reply_elements(), 100_000_000);
        assert_eq!(max_write_values(), 1 << 38);
    }

    #[test]
    fn config_bounds_are_consistent() {
        for limit in &LIMITS {
            assert!(limit.min >= 1 && limit.min <= limit.default && limit.default <= limit.max);
            assert_eq!(limit.value.load(Ordering::Relaxed), limit.default);
        }
        // The write limit can only be lowered from its default.
        assert_eq!(DEFAULT_MAX_WRITE_VALUES, MAX_WRITE_VALUES_CEILING);
    }

    #[test]
    fn config_callbacks_store_and_load() {
        let cell = Box::leak(Box::new(AtomicI64::new(7)));
        let privdata = std::ptr::from_ref(cell).cast_mut().cast();
        unsafe {
            assert_eq!(get_limit(std::ptr::null(), privdata), 7);
            let status = set_limit(std::ptr::null(), 12345, privdata, std::ptr::null_mut());
            assert_eq!(status, raw::REDISMODULE_OK as c_int);
            assert_eq!(get_limit(std::ptr::null(), privdata), 12345);
        }
    }

    #[test]
    fn range_write_bytes_counts_containers() {
        const FULL_SUB: u64 = (1 << 16) * (RECORD_BYTES + RUN_PAYLOAD_BYTES);
        // One container: the vector's minimum capacity of four records.
        assert_eq!(
            range_write_bytes(0, 0),
            4 * RECORD_BYTES + RUN_PAYLOAD_BYTES
        );
        assert_eq!(range_write_bytes(5, 65_535), range_write_bytes(0, 0));
        // Two containers, as soon as the range crosses a 65,536 boundary.
        assert_eq!(
            range_write_bytes(65_535, 65_536),
            4 * RECORD_BYTES + 2 * RUN_PAYLOAD_BYTES
        );
        // R.SETFULL: 65,536 containers, exactly a power of two: 3 MiB.
        assert_eq!(range_write_bytes(0, u64::from(u32::MAX)), FULL_SUB);
        assert_eq!(FULL_SUB, 3 << 20);
        // 100,000 containers: the vector doubles to 131,072 records.
        assert_eq!(
            range_write_bytes(0, (100_000 << 16) - 1),
            131_072 * RECORD_BYTES + 100_000 * RUN_PAYLOAD_BYTES
        );
        // R64: one vector per 2^32 values. 2^38 values from 0 = 64 full
        // sub-bitmaps (about 201 MB).
        assert_eq!(range_write_bytes(0, (1 << 38) - 1), 64 * FULL_SUB);
        assert_eq!(64 * FULL_SUB, 201_326_592);
        // Straddling sub-bitmap borders: two partial vectors, full ones between.
        let first = (3 << 32) - (10 << 16); // last 10 containers of sub 2
        let last = (6 << 32) + (20 << 16) - 1; // first 20 containers of sub 6
        assert_eq!(
            range_write_bytes(first, last),
            vector_bytes(10) + 3 * FULL_SUB + vector_bytes(20)
        );
        // The top of the 64-bit space does not overflow.
        assert!(range_write_bytes(0, u64::MAX) > 1 << 50);
        assert_eq!(
            range_write_bytes(u64::MAX, u64::MAX),
            range_write_bytes(0, 0)
        );
    }

    #[test]
    fn range_write_bytes_matches_the_heap_model() {
        use crate::bitmap_type::RoaringType;
        use roaring::{RoaringBitmap, RoaringTreemap};
        // What a range insert into an empty key actually holds, by the
        // MEMORY USAGE model (struct sizes aside).
        for (start, end) in [(0u32, u32::MAX), (7, 1 << 30), (1 << 20, (1 << 20) + 5)] {
            let mut bm = RoaringBitmap::new();
            bm.insert_range(start..=end);
            let (heap, _, _) = crate::bitmap32::containers_heap_size(&bm);
            let estimate = range_write_bytes(u64::from(start), u64::from(end));
            assert!(
                estimate >= heap as u64,
                "{start}..={end}: {estimate} < {heap}"
            );
            assert!(
                estimate <= 2 * heap as u64,
                "{start}..={end}: {estimate} vs {heap}"
            );
        }
        let mut tm = RoaringTreemap::new();
        tm.insert_range((1 << 32) - 3..(3 << 32) + 70_000);
        let estimate = range_write_bytes((1 << 32) - 3, (3 << 32) + 69_999);
        let heap = RoaringType::heap_size(&tm) as u64;
        assert!(
            estimate <= heap && heap <= estimate + 4_096,
            "{estimate} vs {heap}"
        );
    }

    #[test]
    fn fitting_in_memory() {
        const MB: u64 = 1 << 20;
        // noeviction: used memory plus the write must stay within maxmemory.
        assert!(fits_in_memory(0.5, 1_000 * MB, 400 * MB, false));
        assert!(fits_in_memory(0.5, 1_000 * MB, 500 * MB, false));
        assert!(!fits_in_memory(0.5, 1_000 * MB, 501 * MB, false));
        assert!(!fits_in_memory(0.99, 1_000 * MB, 11 * MB, false));
        assert!(!fits_in_memory(1.2, 1_000 * MB, MB, false));
        // An eviction policy makes room afterwards: only a write larger than
        // maxmemory itself cannot fit.
        assert!(fits_in_memory(0.99, 1_000 * MB, 900 * MB, true));
        assert!(fits_in_memory(1.5, 1_000 * MB, 1_000 * MB, true));
        assert!(!fits_in_memory(0.1, 1_000 * MB, 1_001 * MB, true));
        // No overflow at the extremes.
        assert!(!fits_in_memory(1.0, 1 << 62, 1, false));
        assert!(!fits_in_memory(0.5, 1 << 62, u64::MAX, false));
        assert!(fits_in_memory(0.0, u64::MAX, u64::MAX, true));
    }
}
