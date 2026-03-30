//! valkey-roaring: A Valkey module providing Roaring Bitmap data structures.
//!
//! Registers two custom types (32-bit and 64-bit) and 51 commands.

use std::os::raw::c_void;

use roaring::{RoaringBitmap, RoaringTreemap};
use valkey_module::alloc::ValkeyAlloc;
use valkey_module::native_types::ValkeyType;
use valkey_module::{raw, valkey_module, Context, ValkeyResult, ValkeyString, ValkeyValue, ValkeyError};

mod bitmap32;
mod bitmap64;
mod bitmap_type;
mod commands;
mod commands_bitop;
mod error;
mod parse;

use bitmap_type::RoaringType;

const ENCODING_VERSION: i32 = 1;

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
        aof_rewrite: None, // EmitAOF is varargs C — not wrapped by the Rust SDK
        free: Some(bitmap32_free),
        digest: None,
        mem_usage: Some(bitmap32_mem_usage),
        aux_load: None,
        aux_save: None,
        aux_save2: None,
        aux_save_triggers: 0,
        free_effort: None,
        unlink: None,
        copy: None,
        defrag: None,
        copy2: None,
        free_effort2: None,
        mem_usage2: None,
        unlink2: None,
    },
);

unsafe extern "C" fn bitmap32_rdb_load(
    rdb: *mut raw::RedisModuleIO,
    _encver: i32,
) -> *mut c_void {
    let data = match raw::load_string_buffer(rdb) {
        Ok(buf) => buf,
        Err(_) => return std::ptr::null_mut(),
    };
    match RoaringBitmap::deserialize_from(data.as_ref()) {
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

unsafe extern "C" fn bitmap32_free(value: *mut c_void) {
    drop(Box::from_raw(value as *mut RoaringBitmap));
}

unsafe extern "C" fn bitmap32_mem_usage(value: *const c_void) -> usize {
    let bm = &*(value as *const RoaringBitmap);
    bm.serialized_size()
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
        aof_rewrite: None,
        free: Some(bitmap64_free),
        digest: None,
        mem_usage: Some(bitmap64_mem_usage),
        aux_load: None,
        aux_save: None,
        aux_save2: None,
        aux_save_triggers: 0,
        free_effort: None,
        unlink: None,
        copy: None,
        defrag: None,
        copy2: None,
        free_effort2: None,
        mem_usage2: None,
        unlink2: None,
    },
);

unsafe extern "C" fn bitmap64_rdb_load(
    rdb: *mut raw::RedisModuleIO,
    _encver: i32,
) -> *mut c_void {
    let data = match raw::load_string_buffer(rdb) {
        Ok(buf) => buf,
        Err(_) => return std::ptr::null_mut(),
    };
    match RoaringTreemap::deserialize_from(data.as_ref()) {
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

unsafe extern "C" fn bitmap64_free(value: *mut c_void) {
    drop(Box::from_raw(value as *mut RoaringTreemap));
}

unsafe extern "C" fn bitmap64_mem_usage(value: *const c_void) -> usize {
    let bm = &*(value as *const RoaringTreemap);
    bm.serialized_size()
}

// ============================================================
// R.STAT — shared command detecting type at runtime
// ============================================================
fn handle_stat(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    if args.len() < 2 || args.len() > 3 {
        return Err(ValkeyError::WrongArity);
    }

    let format = if args.len() == 3 {
        args[2].to_string_lossy().to_uppercase()
    } else {
        "TEXT".to_string()
    };

    let key = ctx.open_key(&args[1]);
    if key.is_null() {
        return Ok(ValkeyValue::Null);
    }

    // Try 32-bit type first
    if let Ok(Some(bm)) = key.get_value::<RoaringBitmap>(&BITMAP32_TYPE) {
        let stat = if format == "JSON" {
            bm.stat_json()
        } else {
            bm.stat_text()
        };
        return Ok(ValkeyValue::BulkString(stat));
    }

    // Try 64-bit type
    if let Ok(Some(bm)) = key.get_value::<RoaringTreemap>(&BITMAP64_TYPE) {
        let stat = if format == "JSON" {
            bm.stat_json()
        } else {
            bm.stat_text()
        };
        return Ok(ValkeyValue::BulkString(stat));
    }

    // Key exists but is not a roaring type
    Err(ValkeyError::WrongType)
}

// ============================================================
// Concrete command wrappers — 32-bit
// ============================================================
fn r_setbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setbit::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_getbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getbit::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_getbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getbits::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_clearbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_clearbits::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_clear(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_clear::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_setintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_getintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_appendintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_appendintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_deleteintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_deleteintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_rangeintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_rangeintarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_setbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setbitarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_getbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getbitarray::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_setrange(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setrange::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_setfull(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setfull::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_bitcount(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_bitcount::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_bitpos(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_bitpos::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_min(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_min::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_max(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_max::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_optimize(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_optimize::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_contains(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_contains::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_jaccard(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_jaccard::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_diff(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_diff::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_bitop(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands_bitop::handle_bitop::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_export(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_export::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}
fn r_import(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_import::<RoaringBitmap>(ctx, args, &BITMAP32_TYPE)
}

// ============================================================
// Concrete command wrappers — 64-bit
// ============================================================
fn r64_setbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setbit::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_getbit(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getbit::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_getbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getbits::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_clearbits(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_clearbits::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_clear(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_clear::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_setintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_getintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_appendintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_appendintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_deleteintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_deleteintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_rangeintarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_rangeintarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_setbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setbitarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_getbitarray(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_getbitarray::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_setrange(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setrange::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_setfull(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_setfull::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_bitcount(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_bitcount::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_bitpos(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_bitpos::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_min(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_min::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_max(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_max::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_optimize(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_optimize::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_contains(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_contains::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_jaccard(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_jaccard::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_diff(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_diff::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_bitop(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands_bitop::handle_bitop::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_export(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_export::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}
fn r64_import(ctx: &Context, args: Vec<ValkeyString>) -> ValkeyResult {
    commands::handle_import::<RoaringTreemap>(ctx, args, &BITMAP64_TYPE)
}

// ============================================================
// Module registration
// ============================================================
valkey_module! {
    name: "valkey-roaring",
    version: 1,
    allocator: (ValkeyAlloc, ValkeyAlloc),
    data_types: [
        BITMAP32_TYPE,
        BITMAP64_TYPE,
    ],
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
        ["R.BITOP",           r_bitop,            "write deny-oom",       2, -1, 1],
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
        ["R64.BITOP",         r64_bitop,          "write deny-oom",       2, -1, 1],
        ["R64.EXPORT",        r64_export,         "readonly",             1, 1, 1],
        ["R64.IMPORT",        r64_import,         "write deny-oom",       1, 1, 1],
        // -- Shared command --
        ["R.STAT",            handle_stat,        "readonly",             1, 1, 1],
    ],
}
