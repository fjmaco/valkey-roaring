//! valkey-roaring: A Valkey module providing Roaring Bitmap data structures.
//!
//! Registers two custom types (32-bit and 64-bit) and 51 commands.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::panic::{self, AssertUnwindSafe};

use roaring::{RoaringBitmap, RoaringTreemap};
use valkey_module::alloc::ValkeyAlloc;
use valkey_module::native_types::ValkeyType;
use valkey_module::{
    raw, valkey_module, Context, ModuleOptions, Status, ValkeyError, ValkeyResult, ValkeyString,
    ValkeyValue,
};

mod base64;
mod bitmap32;
mod bitmap64;
mod bitmap_type;
mod canonical;
mod commands;
mod commands_bitop;
mod error;
mod limits;
mod parse;
#[cfg(test)]
mod proptests;
#[cfg(test)]
mod test_util;

use bitmap_type::RoaringType;

/// Internal surface exposed only for the fuzz targets in `fuzz/`.
#[cfg(feature = "fuzzing")]
pub mod fuzzing {
    pub use crate::base64::{decode as base64_decode, encode as base64_encode};
    pub use crate::bitmap_type::RoaringType;
    pub use crate::commands_bitop::{op_and, op_andnot, op_andor, op_one, op_or, op_ornot, op_xor};
}

const ENCODING_VERSION: i32 = 1;

/// AOF rewrite callback body (used without the RDB preamble, i.e.
/// `aof-use-rdb-preamble no`): recreate the key as one `<cmd> key <blob>`.
/// The rewritten AOF starts from an empty dataset, so IMPORT's OR-merge is a
/// plain set. EmitAOF is a C variadic; the SDK leaves it unwrapped, but Rust
/// can call variadic C functions through the raw binding.
///
/// # Safety
/// `aof` and `key` come from Valkey's rewrite loop, and `value` is a live `T`.
unsafe fn emit_import<T: RoaringType>(
    aof: *mut raw::RedisModuleIO,
    cmd: &CStr,
    key: *mut raw::RedisModuleString,
    value: *mut c_void,
) {
    let bm = &*(value as *const T);
    let mut buf = Vec::with_capacity(bm.serialized_size());
    if bm.serialize_into(&mut buf).is_err() {
        return;
    }
    if let Some(emit) = raw::RedisModule_EmitAOF {
        emit(
            aof,
            cmd.as_ptr(),
            c"sb".as_ptr(),
            key,
            buf.as_ptr() as *const c_char,
            buf.len(),
        );
    }
}

// ============================================================
// 32-bit type registration
// ============================================================

pub static BITMAP32_TYPE: ValkeyType = ValkeyType::new(
    "vrroaring",
    ENCODING_VERSION,
    raw::RedisModuleTypeMethods {
        version: raw::REDISMODULE_TYPE_METHOD_VERSION as u64,
        rdb_load: Some(bitmap32_rdb_load),
        rdb_save: Some(bitmap32_rdb_save),
        aof_rewrite: Some(bitmap32_aof_rewrite),
        free: Some(bitmap32_free),
        digest: None,
        mem_usage: Some(bitmap32_mem_usage),
        aux_load: None,
        aux_save: None,
        aux_save2: None,
        aux_save_triggers: 0,
        free_effort: Some(bitmap32_free_effort),
        unlink: None,
        copy: Some(bitmap32_copy),
        defrag: None,
        copy2: None,
        free_effort2: None,
        mem_usage2: None,
        unlink2: None,
    },
);

unsafe extern "C" fn bitmap32_rdb_load(rdb: *mut raw::RedisModuleIO, _encver: i32) -> *mut c_void {
    let data = match raw::load_string_buffer(rdb) {
        Ok(buf) => buf,
        Err(_) => return std::ptr::null_mut(),
    };
    // The saved string is exactly one blob; anything else (trailing bytes
    // included) is a corrupt value, as for R.IMPORT.
    match bitmap_type::decode_exact::<RoaringBitmap>(data.as_ref()) {
        Ok(bm) => Box::into_raw(Box::new(bm)) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn bitmap32_rdb_save(rdb: *mut raw::RedisModuleIO, value: *mut c_void) {
    let bm = &*(value as *const RoaringBitmap);
    let size = bm.serialized_size();
    let mut buf = Vec::with_capacity(size);
    if bm.serialize_into(&mut buf).is_ok() {
        raw::save_slice(rdb, &buf);
    }
}

/// Containers per unit of free effort. Valkey hands a value whose effort
/// exceeds 64 to its lazy-free thread when lazy freeing applies (an
/// overwrite under lazyfree-lazy-server-del, on by default; UNLINK; ...).
/// That costs the main thread about 10 us per value (measured on BITOP and
/// DIFF results of ~150 containers: the job queue, and allocations freed on
/// another thread no longer return to the main thread's cache), about what
/// freeing ~1,000 containers in place costs (~10 ns each). One unit per
/// container sent every value over 64 containers to the background and made
/// mid-size BITOP/DIFF destinations 2-4x slower to overwrite; now values up
/// to 64 * 16 = 1,024 containers are freed in place, as 1.1.1 freed every
/// value, and larger ones (a three-million-container UNLINK) still go to
/// the background.
const CONTAINERS_PER_EFFORT: usize = 16;

/// Free effort for lazy freeing; 0 would mean "always free asynchronously",
/// so at least 1.
fn free_effort<T: RoaringType>(bm: &T) -> usize {
    (bm.container_count() / CONTAINERS_PER_EFFORT).max(1)
}

unsafe extern "C" fn bitmap32_aof_rewrite(
    aof: *mut raw::RedisModuleIO,
    key: *mut raw::RedisModuleString,
    value: *mut c_void,
) {
    emit_import::<RoaringBitmap>(aof, c"R.IMPORT", key, value);
}

unsafe extern "C" fn bitmap32_free(value: *mut c_void) {
    drop(Box::from_raw(value as *mut RoaringBitmap));
}

unsafe extern "C" fn bitmap32_free_effort(
    _key: *mut raw::RedisModuleString,
    value: *const c_void,
) -> usize {
    free_effort(&*(value as *const RoaringBitmap))
}

/// MEMORY USAGE: estimated heap footprint. The serialized size reported
/// before undercounted 1.5-6x (no container records, growth slack or
/// allocation rounding).
unsafe extern "C" fn bitmap32_mem_usage(value: *const c_void) -> usize {
    let bm = &*(value as *const RoaringBitmap);
    bm.heap_size()
}

unsafe extern "C" fn bitmap32_copy(
    _fromkey: *mut raw::RedisModuleString,
    _tokey: *mut raw::RedisModuleString,
    value: *const c_void,
) -> *mut c_void {
    let bm = &*(value as *const RoaringBitmap);
    Box::into_raw(Box::new(bm.clone())) as *mut c_void
}

// ============================================================
// 64-bit type registration
// ============================================================

pub static BITMAP64_TYPE: ValkeyType = ValkeyType::new(
    "vroarng64",
    ENCODING_VERSION,
    raw::RedisModuleTypeMethods {
        version: raw::REDISMODULE_TYPE_METHOD_VERSION as u64,
        rdb_load: Some(bitmap64_rdb_load),
        rdb_save: Some(bitmap64_rdb_save),
        aof_rewrite: Some(bitmap64_aof_rewrite),
        free: Some(bitmap64_free),
        digest: None,
        mem_usage: Some(bitmap64_mem_usage),
        aux_load: None,
        aux_save: None,
        aux_save2: None,
        aux_save_triggers: 0,
        free_effort: Some(bitmap64_free_effort),
        unlink: None,
        copy: Some(bitmap64_copy),
        defrag: None,
        copy2: None,
        free_effort2: None,
        mem_usage2: None,
        unlink2: None,
    },
);

unsafe extern "C" fn bitmap64_rdb_load(rdb: *mut raw::RedisModuleIO, _encver: i32) -> *mut c_void {
    let data = match raw::load_string_buffer(rdb) {
        Ok(buf) => buf,
        Err(_) => return std::ptr::null_mut(),
    };
    // Validated like R.IMPORT: exactly one blob, strictly increasing high
    // words. Empty sub-bitmaps older builds could have stored from an
    // R64.IMPORT are dropped.
    match bitmap_type::decode_exact::<RoaringTreemap>(data.as_ref()) {
        Ok(bm) => Box::into_raw(Box::new(bm)) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn bitmap64_rdb_save(rdb: *mut raw::RedisModuleIO, value: *mut c_void) {
    let bm = &*(value as *const RoaringTreemap);
    let size = bm.serialized_size();
    let mut buf = Vec::with_capacity(size);
    if bm.serialize_into(&mut buf).is_ok() {
        raw::save_slice(rdb, &buf);
    }
}

unsafe extern "C" fn bitmap64_aof_rewrite(
    aof: *mut raw::RedisModuleIO,
    key: *mut raw::RedisModuleString,
    value: *mut c_void,
) {
    emit_import::<RoaringTreemap>(aof, c"R64.IMPORT", key, value);
}

unsafe extern "C" fn bitmap64_free(value: *mut c_void) {
    drop(Box::from_raw(value as *mut RoaringTreemap));
}

unsafe extern "C" fn bitmap64_free_effort(
    _key: *mut raw::RedisModuleString,
    value: *const c_void,
) -> usize {
    free_effort(&*(value as *const RoaringTreemap))
}

unsafe extern "C" fn bitmap64_mem_usage(value: *const c_void) -> usize {
    let bm = &*(value as *const RoaringTreemap);
    bm.heap_size()
}

unsafe extern "C" fn bitmap64_copy(
    _fromkey: *mut raw::RedisModuleString,
    _tokey: *mut raw::RedisModuleString,
    value: *const c_void,
) -> *mut c_void {
    let bm = &*(value as *const RoaringTreemap);
    Box::into_raw(Box::new(bm.clone())) as *mut c_void
}

// ============================================================
// Error normalization
// ============================================================

/// The SDK's verify_type reports type mismatches as a plain string instead of
/// the standard WRONGTYPE error. Normalize so clients can pattern-match the
/// WRONGTYPE prefix.
fn normalize_type_err(e: ValkeyError) -> ValkeyError {
    match e {
        ValkeyError::Str("Existing key has wrong Valkey type") => ValkeyError::WrongType,
        other => other,
    }
}

// ============================================================
// Panic guard
// ============================================================

/// Runs a handler, catching a panic and returning its message. The SDK's
/// command trampoline is a plain `extern "C" fn`, and a panic unwinding out
/// of it aborts the whole server process.
fn catch_panic(handler: impl FnOnce() -> ValkeyResult) -> Result<ValkeyResult, String> {
    panic::catch_unwind(AssertUnwindSafe(handler)).map_err(|payload| {
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("unknown panic");
        // Error replies are single-line; assertion messages are not.
        msg.replace(['\r', '\n'], " ")
    })
}

/// Entry point for every command: a panic in one command becomes one error
/// reply (and a server log line) instead of a server crash.
fn run(ctx: &Context, handler: impl FnOnce() -> ValkeyResult) -> ValkeyResult {
    match catch_panic(handler) {
        Ok(result) => result.map_err(normalize_type_err),
        Err(msg) => {
            ctx.log_warning(&format!("valkey-roaring: command panicked: {msg}"));
            Err(ValkeyError::String(format!(
                "ERR internal error (panic): {msg}"
            )))
        }
    }
}

// ============================================================
// Module init
// ============================================================

/// Writes signal key modification explicitly (commands::key_changed) and
/// only when data actually changed, so no-op writes don't invalidate WATCH
/// or client-side caches. Without this option every key opened for write
/// is signalled on close, changed or not. The reply and write limits are
/// registered as module configuration parameters (limits.rs).
fn init(ctx: &Context, _args: &[ValkeyString]) -> Status {
    ctx.set_module_options(ModuleOptions::NO_IMPLICIT_SIGNAL_MODIFIED);
    limits::register(ctx)
}

// ============================================================
// R.STAT — shared command detecting type at runtime
// ============================================================
fn r_stat(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || handle_stat(ctx, args))
}

fn handle_stat(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() < 2 || args.len() > 3 {
        return Err(ValkeyError::WrongArity);
    }

    // Upstream's layout and units, for either width: JSON only for the exact
    // token "JSON" (anything else is plain text), replied as a verbatim
    // string (a bulk string under RESP2).
    let json = args
        .get(2)
        .is_some_and(|a| parse::c_str(a.as_slice()) == b"JSON");

    let key = ctx.open_key(&args[1]);
    if key.is_null() {
        return Ok(ValkeyValue::Null);
    }

    let fields = if let Ok(Some(bm)) = key.get_value::<RoaringBitmap>(&BITMAP32_TYPE) {
        bm.stat_fields()
    } else if let Ok(Some(bm)) = key.get_value::<RoaringTreemap>(&BITMAP64_TYPE) {
        bm.stat_fields()
    } else {
        // Key exists but is not a roaring type
        return Err(ValkeyError::WrongType);
    };
    let text = if json { fields.json() } else { fields.text() };
    // The format type is not exported by the SDK; built through inference.
    Ok(ValkeyValue::VerbatimString((
        "txt".try_into()?,
        text.into_bytes(),
    )))
}

// ============================================================
// Concrete command wrappers — 32-bit
// ============================================================
fn r_setbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setbit::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_getbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getbit::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_getbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getbits::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_clearbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_clearbits::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_clear(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_clear::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_setintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_getintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_appendintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_appendintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_deleteintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_deleteintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_rangeintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_rangeintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_setbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setbitarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_getbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getbitarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_setrange(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setrange::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_setfull(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setfull::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_bitcount(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_bitcount::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_bitpos(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_bitpos::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_min(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_min::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_max(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_max::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_optimize(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_optimize::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_contains(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_contains::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_jaccard(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_jaccard::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_diff(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_diff::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_bitop(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands_bitop::handle_bitop::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_export(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_export::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}
fn r_import(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_import::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
    })
}

// ============================================================
// Concrete command wrappers — 64-bit
// ============================================================
fn r64_setbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setbit::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_getbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getbit::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_getbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getbits::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_clearbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_clearbits::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_clear(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_clear::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_setintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_getintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_appendintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_appendintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_deleteintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_deleteintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_rangeintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_rangeintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_setbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setbitarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_getbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_getbitarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_setrange(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setrange::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_setfull(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_setfull::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_bitcount(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_bitcount::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_bitpos(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_bitpos::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_min(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_min::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_max(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_max::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_optimize(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_optimize::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_contains(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_contains::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_jaccard(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_jaccard::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_diff(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_diff::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_bitop(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands_bitop::handle_bitop::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_export(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_export::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}
fn r64_import(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    run(ctx, || {
        commands::handle_import::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
    })
}

// ============================================================
// Module registration
// ============================================================
valkey_module! {
    name: "valkey-roaring",
    // major*10000 + minor*100 + patch
    version: 20000,
    allocator: (ValkeyAlloc, ValkeyAlloc),
    data_types: [
        BITMAP32_TYPE,
        BITMAP64_TYPE,
    ],
    init: init,
    commands: [
        // -- 32-bit commands --
        ["R.SETBIT",          r_setbit,          "write fast deny-oom",    1, 1, 1],
        ["R.GETBIT",          r_getbit,          "readonly fast",          1, 1, 1],
        ["R.GETBITS",         r_getbits,         "readonly fast",          1, 1, 1],
        ["R.CLEARBITS",       r_clearbits,       "write fast",             1, 1, 1],
        ["R.CLEAR",           r_clear,           "write",                  1, 1, 1],
        ["R.SETINTARRAY",     r_setintarray,     "write deny-oom",        1, 1, 1],
        ["R.GETINTARRAY",     r_getintarray,     "readonly",              1, 1, 1],
        ["R.APPENDINTARRAY",  r_appendintarray,  "write deny-oom",        1, 1, 1],
        ["R.DELETEINTARRAY",  r_deleteintarray,  "write",                 1, 1, 1],
        ["R.RANGEINTARRAY",   r_rangeintarray,   "readonly",              1, 1, 1],
        ["R.SETBITARRAY",     r_setbitarray,      "write deny-oom",       1, 1, 1],
        ["R.GETBITARRAY",     r_getbitarray,      "readonly",             1, 1, 1],
        ["R.SETRANGE",        r_setrange,         "write deny-oom",       1, 1, 1],
        ["R.SETFULL",         r_setfull,          "write deny-oom",       1, 1, 1],
        ["R.BITCOUNT",        r_bitcount,         "readonly fast",        1, 1, 1],
        ["R.BITPOS",          r_bitpos,           "readonly",             1, 1, 1],
        ["R.MIN",             r_min,              "readonly fast",        1, 1, 1],
        ["R.MAX",             r_max,              "readonly fast",        1, 1, 1],
        ["R.OPTIMIZE",        r_optimize,         "write",                1, 1, 1],
        ["R.CONTAINS",        r_contains,         "readonly",             1, 2, 1],
        ["R.JACCARD",         r_jaccard,          "readonly",             1, 2, 1],
        ["R.DIFF",            r_diff,             "write deny-oom",       1, 3, 1],
        // getkeys-api: NOT takes a trailing non-key `last` arg, so key positions
        // are reported dynamically (see commands_bitop::report_bitop_keys).
        ["R.BITOP",           r_bitop,            "write deny-oom getkeys-api", 2, 2, 1],
        ["R.EXPORT",          r_export,           "readonly",             1, 1, 1],
        ["R.IMPORT",          r_import,           "write deny-oom",       1, 1, 1],
        // -- 64-bit commands --
        ["R64.SETBIT",        r64_setbit,         "write fast deny-oom",  1, 1, 1],
        ["R64.GETBIT",        r64_getbit,         "readonly fast",        1, 1, 1],
        ["R64.GETBITS",       r64_getbits,        "readonly fast",        1, 1, 1],
        ["R64.CLEARBITS",     r64_clearbits,      "write fast",           1, 1, 1],
        ["R64.CLEAR",         r64_clear,          "write",                1, 1, 1],
        ["R64.SETINTARRAY",   r64_setintarray,    "write deny-oom",       1, 1, 1],
        ["R64.GETINTARRAY",   r64_getintarray,    "readonly",             1, 1, 1],
        ["R64.APPENDINTARRAY", r64_appendintarray, "write deny-oom",      1, 1, 1],
        ["R64.DELETEINTARRAY", r64_deleteintarray, "write",               1, 1, 1],
        ["R64.RANGEINTARRAY", r64_rangeintarray,  "readonly",             1, 1, 1],
        ["R64.SETBITARRAY",   r64_setbitarray,    "write deny-oom",       1, 1, 1],
        ["R64.GETBITARRAY",   r64_getbitarray,    "readonly",             1, 1, 1],
        ["R64.SETRANGE",      r64_setrange,       "write deny-oom",       1, 1, 1],
        ["R64.SETFULL",       r64_setfull,        "write deny-oom",       1, 1, 1],
        ["R64.BITCOUNT",      r64_bitcount,       "readonly fast",        1, 1, 1],
        ["R64.BITPOS",        r64_bitpos,         "readonly",             1, 1, 1],
        ["R64.MIN",           r64_min,            "readonly fast",        1, 1, 1],
        ["R64.MAX",           r64_max,            "readonly fast",        1, 1, 1],
        ["R64.OPTIMIZE",      r64_optimize,       "write",                1, 1, 1],
        ["R64.CONTAINS",      r64_contains,       "readonly",             1, 2, 1],
        ["R64.JACCARD",       r64_jaccard,        "readonly",             1, 2, 1],
        ["R64.DIFF",          r64_diff,           "write deny-oom",       1, 3, 1],
        ["R64.BITOP",         r64_bitop,          "write deny-oom getkeys-api", 2, 2, 1],
        ["R64.EXPORT",        r64_export,         "readonly",             1, 1, 1],
        ["R64.IMPORT",        r64_import,         "write deny-oom",       1, 1, 1],
        // -- Shared command --
        ["R.STAT",            r_stat,             "readonly",             1, 1, 1],
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_panic_turns_panics_into_messages() {
        assert_eq!(
            catch_panic(|| Ok(ValkeyValue::Integer(1)))
                .unwrap()
                .unwrap(),
            ValkeyValue::Integer(1)
        );
        assert_eq!(catch_panic(|| panic!("boom")).unwrap_err(), "boom");

        let empty: Vec<u32> = Vec::new();
        let msg = catch_panic(|| Ok(ValkeyValue::Integer(i64::from(empty[3])))).unwrap_err();
        assert!(msg.contains("index out of bounds"), "{msg}");

        // Multi-line assertion messages become one line for the error reply.
        let msg = catch_panic(|| {
            assert_eq!(1 + 1, 3);
            Ok(ValkeyValue::Null)
        })
        .unwrap_err();
        assert!(msg.contains("left") && !msg.contains('\n'), "{msg}");
    }

    #[test]
    fn free_effort_is_never_zero() {
        // 0 means "always free asynchronously" to the server.
        assert_eq!(free_effort(&RoaringBitmap::new()), 1);
        assert_eq!(free_effort(&RoaringTreemap::new()), 1);
        let spread: RoaringBitmap = (0..100u32).map(|i| i << 16).collect();
        assert_eq!(free_effort(&spread), 6);
        // Freed in place up to 1,024 containers (effort 64), in the
        // background beyond.
        let at: RoaringBitmap = (0..1_024u32).map(|i| i << 16).collect();
        assert_eq!(free_effort(&at), 64);
        let over: RoaringBitmap = (0..1_040u32).map(|i| i << 16).collect();
        assert!(free_effort(&over) > 64);
        let huge: RoaringTreemap = (0..3_000_000u64).map(|i| i << 16).collect();
        assert_eq!(free_effort(&huge), 3_000_000 / 16);
    }
}
