//! valkey-roaring: Generic command handlers parameterized by RoaringType.

use crate::bitmap_type::{decode_exact, RoaringType};
use crate::error::*;
use crate::parse::*;
use std::ffi::CString;
use std::os::raw::c_long;
use valkey_module::native_types::ValkeyType;
use valkey_module::{
    raw, Context, ContextFlags, ValkeyError, ValkeyResult, ValkeyString, ValkeyValue,
};

// ============================================================
// Helper: publish a write that changed a key
// ============================================================
/// The module runs with NO_IMPLICIT_SIGNAL_MODIFIED (see lib.rs), so opening
/// a key for write has no visible effect on its own. Handlers call this only
/// after a real change: it invalidates WATCH and client-side caching for the
/// key and propagates the command to replicas and the AOF, which also counts
/// toward RDB save points. Writes that turn out to be no-ops skip it.
pub(crate) fn key_changed(ctx: &Context, key: &ValkeyString) {
    // SAFETY: both pointers come from the live command context.
    unsafe {
        if let Some(signal) = raw::RedisModule_SignalModifiedKey {
            signal(ctx.get_raw(), key.inner);
        }
    }
    ctx.replicate_verbatim();
}

// ============================================================
// Helper: get or create bitmap from a writable key
// ============================================================
/// Returns the bitmap and whether this call created the key.
fn get_or_create<'a, T: RoaringType>(
    key: &'a valkey_module::key::ValkeyKeyWritable,
    vtype: &ValkeyType,
) -> Result<(&'a mut T, bool), ValkeyError> {
    let created = key.get_value::<T>(vtype)?.is_none();
    if created {
        key.set_value(vtype, T::new())?;
    }
    Ok((key.get_value::<T>(vtype)?.unwrap(), created))
}

fn require_existing<'a, T: RoaringType>(
    key: &'a valkey_module::key::ValkeyKey,
    vtype: &ValkeyType,
) -> Result<&'a T, ValkeyError> {
    key.get_value::<T>(vtype)?
        .ok_or(ValkeyError::Str(ERR_KEY_NOT_FOUND))
}

/// Reject a key of another type (WRONGTYPE) before anything else, as
/// upstream does: it opens the key, then parses arguments.
pub(crate) fn check_type<T: RoaringType>(
    ctx: &Context,
    key: &ValkeyString,
    vtype: &ValkeyType,
) -> Result<(), ValkeyError> {
    ctx.open_key(key).get_value::<T>(vtype).map(|_| ())
}

// ============================================================
// Value parsing — upstream's grammar and error text per width
// ============================================================
/// Whether the command is being replayed from the AOF or came from the
/// primary, rather than from a client. Both carry commands exactly as the
/// build that wrote them accepted them: valkey-roaring 1.1.1 took inputs
/// the strict grammar now refuses ("+5", "01", lowercase BITOP operations,
/// IMPORT blobs with trailing bytes or unordered 64-bit high words), and
/// refusing them on replay would silently drop keys after an upgrade. So
/// input the strict rules refuse is read with 1.1.1's rules there. A newer
/// primary or AOF only carries strictly valid input, which reads the same
/// either way.
///
/// Asked only after a strict parse fails: the flags call is not free (the
/// server also works out its maxmemory state for it).
pub(crate) fn from_aof_or_primary(ctx: &Context) -> bool {
    ctx.get_flags()
        .intersects(ContextFlags::LOADING | ContextFlags::REPLICATED)
}

pub(crate) fn parse_value<T: RoaringType>(
    ctx: &Context,
    arg: &ValkeyString,
    name: &str,
) -> Result<T::Value, ValkeyError> {
    let bytes = arg.as_slice();
    T::parse_value_arg(bytes)
        .or_else(|| {
            from_aof_or_primary(ctx)
                .then(|| T::parse_value_legacy(bytes))
                .flatten()
        })
        .ok_or_else(|| {
            ValkeyError::String(format!("ERR invalid {}: {}", name, T::VALUE_DESCRIPTION))
        })
}

fn parse_values<T: RoaringType>(
    ctx: &Context,
    args: &[ValkeyString],
    name: &str,
) -> Result<Vec<T::Value>, ValkeyError> {
    args.iter()
        .map(|a| parse_value::<T>(ctx, a, name))
        .collect()
}

fn parse_bit(ctx: &Context, arg: &ValkeyString, name: &str) -> Result<bool, ValkeyError> {
    let bytes = arg.as_slice();
    parse_bit_strict(bytes)
        .or_else(|| {
            from_aof_or_primary(ctx)
                .then(|| parse_bit_legacy(bytes))
                .flatten()
        })
        .ok_or_else(|| ValkeyError::String(format!("ERR invalid {}: must be either 0 or 1", name)))
}

/// Reply with a bitmap value. Values that fit i64 are integer replies; larger
/// u64 values are decimal bulk strings, matching the C module's ReplyWithUint64.
pub(crate) fn value_reply<T: RoaringType>(v: T::Value) -> ValkeyValue {
    let i = T::value_to_i64(v); // saturates at i64::MAX
    if i == i64::MAX && v.to_string() != i.to_string() {
        ValkeyValue::BulkString(v.to_string())
    } else {
        ValkeyValue::Integer(i)
    }
}

// ============================================================
// Helper: stream array replies
// ============================================================
// Large array replies go straight into the client's reply buffer, element by
// element, through the same RedisModule_ReplyWith* calls the SDK would make
// for a ValkeyValue::Array, so the bytes on the wire are identical. Building
// the Vec<ValkeyValue> first costs one 56-byte enum per element on top of the
// reply itself (56 MB for a million-value GETINTARRAY) and a second pass.
// Everything that can fail must run before the array header goes out: after
// it, the reply has to be completed with exactly `len` elements.

fn reply_value<T: RoaringType>(ctx: &Context, v: T::Value) {
    match value_reply::<T>(v) {
        ValkeyValue::Integer(i) => raw::reply_with_long_long(ctx.get_raw(), i),
        other => ctx.reply(Ok(other)),
    };
}

/// Streams `len` bitmap values as an array reply. `values` must yield at
/// least `len` items.
fn reply_value_array<T: RoaringType>(
    ctx: &Context,
    len: u64,
    values: impl Iterator<Item = T::Value>,
) -> ValkeyResult {
    raw::reply_with_array(ctx.get_raw(), len as c_long);
    for v in values.take(len as usize) {
        reply_value::<T>(ctx, v);
    }
    Ok(ValkeyValue::NoReply)
}

/// Streams every value of `values` as an array reply whose length is set
/// once they are all out (a postponed length, still an ordinary
/// `*<count>` header on the wire), for callers that would otherwise need a
/// separate pass to count them.
fn reply_value_array_counted<T: RoaringType>(
    ctx: &Context,
    values: impl Iterator<Item = T::Value>,
) -> ValkeyResult {
    // SAFETY: read once; the API table is filled at module load.
    let set_length = unsafe { raw::RedisModule_ReplySetArrayLength }.ok_or(ValkeyError::Str(
        "ERR RedisModule_ReplySetArrayLength unavailable",
    ))?;
    raw::reply_with_array(ctx.get_raw(), raw::REDISMODULE_POSTPONED_LEN as c_long);
    let mut count: c_long = 0;
    for v in values {
        reply_value::<T>(ctx, v);
        count += 1;
    }
    // SAFETY: the context is live and the postponed array was just opened.
    unsafe { set_length(ctx.get_raw(), count) };
    Ok(ValkeyValue::NoReply)
}

// ============================================================
// Upstream reply formats
// ============================================================
/// CONTAINS's error text for an unknown mode, as upstream's reply reaches
/// the client: the argument's raw bytes as a C string (up to a NUL byte),
/// the message cut to 255 bytes by its 256-byte vsnprintf buffer, then what
/// the server does to every error text: trailing CR/LF trimmed, any other
/// CR/LF turned into spaces.
fn invalid_mode_message(mode: &[u8]) -> CString {
    let mut msg = b"ERR invalid mode argument: ".to_vec();
    msg.extend_from_slice(c_str(mode));
    msg.truncate(255);
    while msg.last().is_some_and(|&b| b == b'\r' || b == b'\n') {
        msg.pop();
    }
    for b in &mut msg {
        if *b == b'\r' || *b == b'\n' {
            *b = b' ';
        }
    }
    CString::new(msg).expect("no NUL after c_str")
}

/// Replies the unknown-mode error through the raw API, so the argument's
/// bytes go out as they came: the SDK's error path needs UTF-8 text.
fn reply_invalid_mode(ctx: &Context, mode: &[u8]) -> ValkeyResult {
    let msg = invalid_mode_message(mode);
    // SAFETY: the API table is filled at module load; `msg` is
    // NUL-terminated and outlives the call, which copies it.
    unsafe {
        match raw::RedisModule_ReplyWithError {
            Some(reply) => reply(ctx.get_raw(), msg.as_ptr()),
            None => return Err(ValkeyError::Str("ERR invalid mode argument")),
        };
    }
    Ok(ValkeyValue::NoReply)
}

/// JACCARD's reply, exactly as upstream writes it (a bulk string in both
/// RESP versions): -1 for two empty sets, 0, 1, the exact decimal when the
/// ratio has one within nine fractional digits, else printf's %.17g.
pub(crate) fn jaccard_text(intersection: u64, union: u64) -> String {
    if union == 0 {
        return "-1".to_string();
    }
    if intersection == 0 {
        return "0".to_string();
    }
    if intersection == union {
        return "1".to_string();
    }
    let mut scaled = intersection;
    for scale in 1..=9u32 {
        if scaled > u64::MAX / 10 {
            break;
        }
        scaled *= 10;
        if scaled.is_multiple_of(union) {
            let value = scaled / union;
            let unit = 10u64.pow(scale);
            return format!(
                "{}.{:0width$}",
                value / unit,
                value % unit,
                width = scale as usize
            );
        }
    }
    format_g17(intersection as f64 / union as f64)
}

/// printf("%.17g") for a positive finite double: 17 significant digits,
/// exponent form below 1e-4 or from 1e17, trailing zeros dropped, the
/// exponent written with a sign and at least two digits. Rust's precision
/// formatting is correctly rounded on the exact binary value, as glibc's is.
pub(crate) fn format_g17(x: f64) -> String {
    fn trim(s: &str) -> &str {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.')
        } else {
            s
        }
    }
    let sci = format!("{:.16e}", x);
    let (mantissa, exp) = sci.split_once('e').expect("exponent form");
    let exp: i32 = exp.parse().expect("integer exponent");
    if !(-4..17).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", trim(mantissa), sign, exp.abs())
    } else {
        trim(&format!("{:.*}", (16 - exp) as usize, x)).to_string()
    }
}

// ============================================================
// R.SETBIT / R64.SETBIT
// ============================================================
pub fn handle_setbit<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    key.get_value::<T>(vtype)?; // WRONGTYPE first
    let offset = parse_value::<T>(ctx, &args[2], "offset")?;
    let value = parse_bit(ctx, &args[3], "value")?;
    let (bitmap, created) = get_or_create::<T>(&key, vtype)?;

    // insert/remove report whether the bit flipped, which also gives the
    // previous value without a separate lookup. Setting a bit to the value it
    // already holds is a no-op: nothing is replicated, dirtied or
    // invalidated. A missing key is still created (empty for value 0), as
    // upstream creates it.
    let flipped = if value {
        bitmap.insert(offset)
    } else {
        bitmap.remove(offset)
    };
    let previous = flipped != value;
    if flipped || created {
        key_changed(ctx, &args[1]);
    }

    Ok(ValkeyValue::Integer(previous as i64))
}

// ============================================================
// R.GETBIT / R64.GETBIT
// ============================================================
pub fn handle_getbit<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    let bitmap = key.get_value::<T>(vtype)?;
    // Upstream's R.GETBIT answers 0 for a missing key without looking at the
    // offset; its R64.GETBIT validates the offset first.
    if bitmap.is_none() && !T::GETBIT_PARSES_BEFORE_KEY_CHECK {
        return Ok(ValkeyValue::Integer(0));
    }
    let offset = parse_value::<T>(ctx, &args[2], "offset")?;
    Ok(ValkeyValue::Integer(
        bitmap.is_some_and(|b| b.contains(offset)) as i64,
    ))
}

// ============================================================
// R.GETBITS / R64.GETBITS
// ============================================================
pub fn handle_getbits<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    // A missing key replies an empty array (redis-roaring semantics), not
    // a zero per offset, and before the offsets are parsed.
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let offsets = parse_values::<T>(ctx, &args[2..], "offset")?;
            raw::reply_with_array(ctx.get_raw(), offsets.len() as c_long);
            for &offset in &offsets {
                raw::reply_with_long_long(ctx.get_raw(), bitmap.contains(offset) as i64);
            }
            Ok(ValkeyValue::NoReply)
        }
        None => Ok(ValkeyValue::Array(vec![])),
    }
}

// ============================================================
// R.CLEARBITS / R64.CLEARBITS
// ============================================================
pub fn handle_clearbits<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            // A trailing literal COUNT switches the reply from OK to the
            // number of bits actually cleared (redis-roaring semantics).
            let mut offset_args = &args[2..];
            let count_mode = offset_args
                .last()
                .is_some_and(|a| c_str(a.as_slice()) == b"COUNT");
            if count_mode {
                offset_args = &offset_args[..offset_args.len() - 1];
            }
            let offsets = parse_values::<T>(ctx, offset_args, "offset")?;
            let count = bitmap.remove_many(&offsets);
            if count > 0 {
                key_changed(ctx, &args[1]);
            }
            if count_mode {
                Ok(ValkeyValue::Integer(count as i64))
            } else {
                Ok(ValkeyValue::SimpleStringStatic("OK"))
            }
        }
        None => Ok(ValkeyValue::Null),
    }
}

// ============================================================
// R.CLEAR / R64.CLEAR
// ============================================================
pub fn handle_clear<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let card = bitmap.len();
            if card > 0 {
                bitmap.clear();
                key_changed(ctx, &args[1]);
            }
            Ok(ValkeyValue::Integer(card as i64))
        }
        None => Ok(ValkeyValue::Null),
    }
}

// ============================================================
// R.SETINTARRAY / R64.SETINTARRAY
// ============================================================
pub fn handle_setintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    key.get_value::<T>(vtype)?; // WRONGTYPE first
    let vals = parse_values::<T>(ctx, &args[2..], "value")?;
    // Built with every container at exact size: no compacting copy needed.
    let bm = T::from_values(vals);
    key.set_value(vtype, bm)?;
    key_changed(ctx, &args[1]);

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.GETINTARRAY / R64.GETINTARRAY
// ============================================================
pub fn handle_getintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            // The same cap as RANGEINTARRAY: a key past it (R.SETFULL, a NOT
            // near the top of the range) would stream billions of elements
            // into the reply buffer until the server is killed.
            let card = bitmap.len();
            if card > MAX_RANGE_SIZE {
                return Err(ValkeyError::Str(ERR_RANGE_TOO_LARGE));
            }
            reply_value_array::<T>(ctx, card, bitmap.iter_values())
        }
        None => Ok(ValkeyValue::Array(vec![])),
    }
}

// ============================================================
// R.APPENDINTARRAY / R64.APPENDINTARRAY
// ============================================================
pub fn handle_appendintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    key.get_value::<T>(vtype)?; // WRONGTYPE first
    let vals = parse_values::<T>(ctx, &args[2..], "value")?;
    let (bitmap, created) = get_or_create::<T>(&key, vtype)?;
    if bitmap.insert_many(&vals) > 0 || created {
        key_changed(ctx, &args[1]);
    }

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.DELETEINTARRAY / R64.DELETEINTARRAY
// ============================================================
pub fn handle_deleteintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let vals = parse_values::<T>(ctx, &args[2..], "value")?;
            if bitmap.remove_many(&vals) > 0 {
                key_changed(ctx, &args[1]);
            }
        }
        None => {
            // A missing key is created empty, without looking at the values,
            // as upstream does.
            key.set_value(vtype, T::new())?;
            key_changed(ctx, &args[1]);
        }
    }

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.RANGEINTARRAY / R64.RANGEINTARRAY
// ============================================================
pub fn handle_rangeintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    let bitmap = key.get_value::<T>(vtype)?;
    let start = parse_value::<T>(ctx, &args[2], "start")?;
    let end = parse_value::<T>(ctx, &args[3], "end")?;

    // start/end are 0-based POSITIONS in the sorted value array (pagination),
    // matching redis-roaring: elements at indexes [start, end], truncated at
    // the cardinality. An inverted range replies empty.
    let (start, end) = (T::value_to_u64(start), T::value_to_u64(end));
    if start > end {
        return Ok(ValkeyValue::Array(vec![]));
    }
    // Upstream sizes the range as end - start + 1 in the value width, so the
    // one full-width request (0 to the width's maximum) wraps to 0 and skips
    // the cap: it lists the whole set. Kept, except that a set past the cap
    // is refused (upstream would try to list billions of values; its 64-bit
    // variant fails the allocation and replies "ERR out of memory" instead).
    let full_width = start == 0 && end == T::value_to_u64(T::MAX_VALUE);
    if end - start >= MAX_RANGE_SIZE && !full_width {
        return Err(ValkeyError::Str(ERR_RANGE_TOO_LARGE));
    }

    match bitmap {
        Some(bitmap) if full_width && bitmap.len() > MAX_RANGE_SIZE => {
            Err(ValkeyError::Str(ERR_RANGE_TOO_LARGE))
        }
        Some(bitmap) => {
            // One select for the first position, then a plain walk: select
            // costs a pass over the containers, so calling it per element
            // made a page cost O(page * containers). select stops at `start`
            // (None past the end), and the page is counted as it streams, so
            // no cardinality pass is needed either.
            match bitmap.select(start) {
                Some(first) => reply_value_array_counted::<T>(
                    ctx,
                    // Saturating: the full-width request spans 2^64 positions.
                    bitmap.iter_from(first).take(
                        usize::try_from((end - start).saturating_add(1)).unwrap_or(usize::MAX),
                    ),
                ),
                None => Ok(ValkeyValue::Array(vec![])),
            }
        }
        None => Ok(ValkeyValue::Array(vec![])),
    }
}

// ============================================================
// R.SETBITARRAY / R64.SETBITARRAY
// ============================================================
pub fn handle_setbitarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    key.get_value::<T>(vtype)?; // WRONGTYPE first
                                // Byte i set to '1' sets bit i, over the raw bytes like upstream (a
                                // lossy UTF-8 copy turned each invalid byte into three, shifting later
                                // bits), built with every container at exact size.
    let bm = T::from_bit_array(args[2].as_slice());
    key.set_value(vtype, bm)?;
    key_changed(ctx, &args[1]);

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.GETBITARRAY / R64.GETBITARRAY
// ============================================================
pub fn handle_getbitarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            // The reply is max+1 bytes; refuse instead of risking an
            // allocation-failure abort on huge maxima (upstream crashes).
            let max = bitmap.max_val().map_or(0, T::value_to_u64);
            if max >= MAX_RANGE_SIZE {
                return Err(ValkeyError::Str(ERR_RANGE_TOO_LARGE));
            }
            // An empty bitmap reads "0", as upstream's max-plus-one string.
            let bits = match bitmap.max_val() {
                Some(_) => bitmap.to_bit_array(),
                None => b"0".to_vec(),
            };
            let s = String::from_utf8(bits).unwrap_or_default();
            Ok(ValkeyValue::BulkString(s))
        }
        // Upstream replies an empty simple string for a missing key.
        None => Ok(ValkeyValue::SimpleStringStatic("")),
    }
}

// ============================================================
// R.SETRANGE / R64.SETRANGE
// ============================================================
pub fn handle_setrange<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    key.get_value::<T>(vtype)?; // WRONGTYPE first
    let start = parse_value::<T>(ctx, &args[2], "start")?;
    let end = parse_value::<T>(ctx, &args[3], "end")?;

    if end < start {
        return Err(ValkeyError::Str(T::ERR_END_BEFORE_START));
    }
    if T::value_to_u64(end) - T::value_to_u64(start) > MAX_STORED_RANGE {
        return Err(ValkeyError::Str(ERR_STORED_RANGE_TOO_LARGE));
    }

    let (bitmap, created) = get_or_create::<T>(&key, vtype)?;
    // End-exclusive [start, end), matching redis-roaring / CRoaring add_range.
    // An already-set range is skipped outright rather than inserted and
    // judged by its count: roaring-rs re-shapes a full bitmap container into
    // a run container even when nothing is added, and a no-op should leave
    // the value untouched.
    let mut changed = created;
    if !bitmap.contains_range_exclusive(start, end) {
        bitmap.insert_range_exclusive(start, end);
        changed = true;
    }
    if changed {
        key_changed(ctx, &args[1]);
    }

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.SETFULL / R64.SETFULL
// ============================================================
pub fn handle_setfull<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    if key.get_value::<T>(vtype)?.is_some() {
        return Err(ValkeyError::Str(ERR_KEY_EXISTS));
    }
    // R.SETFULL (2^32 values) is stored as 65,536 run containers; the full
    // 64-bit space cannot be stored at all.
    if T::value_to_u64(T::MAX_VALUE) >= MAX_STORED_RANGE {
        return Err(ValkeyError::Str(ERR_STORED_RANGE_TOO_LARGE));
    }

    let bm = T::full();
    key.set_value(vtype, bm)?;
    key_changed(ctx, &args[1]);

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.BITCOUNT / R64.BITCOUNT
// ============================================================
pub fn handle_bitcount<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => Ok(ValkeyValue::Integer(bitmap.len() as i64)),
        None => Ok(ValkeyValue::Integer(0)),
    }
}

// ============================================================
// R.BITPOS / R64.BITPOS
// ============================================================
pub fn handle_bitpos<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    let bitmap = key.get_value::<T>(vtype)?;
    let bit = parse_bit(ctx, &args[2], "bit")?;

    match bitmap {
        Some(bitmap) => {
            if bit {
                // First set bit
                // The minimum: select(0) would size a whole R64 sub-bitmap.
                match bitmap.min_val() {
                    Some(v) => Ok(value_reply::<T>(v)),
                    None => Ok(ValkeyValue::Integer(-1)),
                }
            } else {
                // First unset bit
                match bitmap.nth_absent(1) {
                    Some(v) => Ok(value_reply::<T>(v)),
                    None => Ok(ValkeyValue::Integer(-1)),
                }
            }
        }
        None => {
            if bit {
                Ok(ValkeyValue::Integer(-1))
            } else {
                // Empty bitmap: first absent bit is 0
                Ok(ValkeyValue::Integer(0))
            }
        }
    }
}

// ============================================================
// R.MIN / R64.MIN
// ============================================================
pub fn handle_min<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => match bitmap.min_val() {
            Some(v) => Ok(value_reply::<T>(v)),
            None => Ok(ValkeyValue::Integer(-1)),
        },
        None => Ok(ValkeyValue::Integer(-1)),
    }
}

// ============================================================
// R.MAX / R64.MAX
// ============================================================
pub fn handle_max<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => match bitmap.max_val() {
            Some(v) => Ok(value_reply::<T>(v)),
            None => Ok(ValkeyValue::Integer(-1)),
        },
        None => Ok(ValkeyValue::Integer(-1)),
    }
}

// ============================================================
// R.OPTIMIZE / R64.OPTIMIZE
// ============================================================
pub fn handle_optimize<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 2 || args.len() > 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            // Optimize the encoding, then drop the growth slack incremental
            // writes leave in container vectors when it is worth a copy
            // (trim's policy): compacting unconditionally made OPTIMIZE of a
            // small sparse key 2-4x slower than upstream for no saving.
            bitmap.optimize();
            bitmap.trim();
            key_changed(ctx, &args[1]);
            Ok(ValkeyValue::SimpleStringStatic("OK"))
        }
        // Upstream requires the key (its optional MEM argument is accepted
        // and, like any other third argument, changes nothing here).
        None => Err(ValkeyError::Str(ERR_KEY_NOT_FOUND)),
    }
}

// ============================================================
// R.CONTAINS / R64.CONTAINS
// ============================================================
pub fn handle_contains<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 || args.len() > 4 {
        return Err(ValkeyError::WrongArity);
    }

    let key1 = ctx.open_key(&args[1]);
    let b1 = require_existing::<T>(&key1, vtype)?;
    let key2 = ctx.open_key(&args[2]);
    let b2 = require_existing::<T>(&key2, vtype)?;

    // Modes are matched exactly, as upstream's strcmp does (no lowercase,
    // and "NONE" is only the implicit default, never a token).
    let result = match args.get(3).map(|a| c_str(a.as_slice())) {
        None => !b1.is_disjoint(b2),
        Some(b"ALL") => b2.is_subset(b1),
        Some(b"ALL_STRICT") => b2.is_subset(b1) && !b1.set_eq(b2),
        Some(b"EQ") => b1.set_eq(b2),
        Some(mode) => return reply_invalid_mode(ctx, mode),
    };

    Ok(ValkeyValue::Integer(result as i64))
}

// ============================================================
// R.JACCARD / R64.JACCARD
// ============================================================
pub fn handle_jaccard<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }

    let key1 = ctx.open_key(&args[1]);
    let b1 = require_existing::<T>(&key1, vtype)?;
    let key2 = ctx.open_key(&args[2]);
    let b2 = require_existing::<T>(&key2, vtype)?;

    // One intersection pass serves both terms: |A ∪ B| = |A| + |B| - |A ∩ B|.
    // roaring's union_len computes exactly this (wrapping, as here), so
    // calling it as well walked the intersection twice.
    let intersection = b1.intersection_len(b2);
    let union = b1.len().wrapping_add(b2.len()).wrapping_sub(intersection);
    Ok(ValkeyValue::BulkString(jaccard_text(intersection, union)))
}

// ============================================================
// R.DIFF / R64.DIFF (separate command, not BITOP DIFF)
// ============================================================
pub fn handle_diff<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }

    // Upstream checks the destination's type before the sources.
    check_type::<T>(ctx, &args[1], vtype)?;
    // Compute from borrowed sources; their keys close before the
    // destination opens, so dest may alias a source.
    let mut result = {
        let key1 = ctx.open_key(&args[2]);
        let b1 = require_existing::<T>(&key1, vtype)?;
        let key2 = ctx.open_key(&args[3]);
        let b2 = require_existing::<T>(&key2, vtype)?;
        b1.difference(b2)
    };
    result.trim();

    let dest = ctx.open_key_writable(&args[1]);
    dest.set_value(vtype, result)?;
    key_changed(ctx, &args[1]);

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.EXPORT / R64.EXPORT
// ============================================================
pub fn handle_export<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    let bitmap = match key.get_value::<T>(vtype)? {
        Some(bm) => bm,
        None => return Err(ValkeyError::Str(ERR_KEY_NOT_FOUND)),
    };

    // The blob is canonical: one set, one byte sequence, whatever writes
    // built it (consumers may hash or dedupe blobs). Getting there optimizes
    // the stored value in place; that only re-shapes containers, never the
    // set, so it is deliberately not signalled or replicated: an export never
    // invalidates WATCH or client-side caches.
    let buf = bitmap
        .export_canonical()
        .map_err(|_| ValkeyError::Str("ERR serialization failed"))?;

    Ok(ValkeyValue::StringBuffer(buf))
}

// ============================================================
// R.IMPORT / R64.IMPORT
// ============================================================
pub fn handle_import<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    // One complete, valid blob: trailing bytes, malformed containers and
    // (64-bit) repeated or decreasing high words are refused, never dropped
    // or truncated. Replayed or replicated commands may still carry what
    // 1.1.1 accepted, and are decoded as it decoded them (see
    // from_aof_or_primary).
    let data = args[2].as_slice();
    let new_bitmap = match decode_exact::<T>(data) {
        Ok(bm) => bm,
        Err(_) if from_aof_or_primary(ctx) => {
            T::deserialize_legacy(data).map_err(|_| ValkeyError::Str(ERR_BAD_BINARY))?
        }
        Err(_) => return Err(ValkeyError::Str(ERR_BAD_BINARY)),
    };

    let key = ctx.open_key_writable(&args[1]);
    // Reply: cardinality after import.
    let card = match key.get_value::<T>(vtype)? {
        Some(existing) => {
            // OR-merge into the existing key. A blob that adds nothing is a
            // no-op: skip the merge itself, not only the replication (a union
            // can re-shape containers without adding values; see SETRANGE).
            if !new_bitmap.is_subset(existing) {
                existing.bitor_assign_owned(new_bitmap);
                key_changed(ctx, &args[1]);
            }
            existing.len()
        }
        None => {
            let card = new_bitmap.len();
            key.set_value(vtype, new_bitmap)?;
            key_changed(ctx, &args[1]);
            card
        }
    };

    Ok(ValkeyValue::Integer(card as i64))
}

// ============================================================
// R.STAT (shared handler — detects type at runtime)
// This is implemented in lib.rs since it needs both types.
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use roaring::{RoaringBitmap, RoaringTreemap};

    #[test]
    fn value_reply_integer_for_representable_values() {
        assert_eq!(value_reply::<RoaringBitmap>(0), ValkeyValue::Integer(0));
        assert_eq!(value_reply::<RoaringBitmap>(5), ValkeyValue::Integer(5));
        assert_eq!(
            value_reply::<RoaringBitmap>(u32::MAX),
            ValkeyValue::Integer(u32::MAX as i64)
        );
        assert_eq!(
            value_reply::<RoaringTreemap>(i64::MAX as u64),
            ValkeyValue::Integer(i64::MAX)
        );
    }

    #[test]
    fn jaccard_text_matches_upstream_formatting() {
        assert_eq!(jaccard_text(0, 0), "-1");
        assert_eq!(jaccard_text(0, 5), "0");
        assert_eq!(jaccard_text(5, 5), "1");
        assert_eq!(jaccard_text(2, 5), "0.4");
        assert_eq!(jaccard_text(1, 4), "0.25");
        assert_eq!(jaccard_text(1, 8), "0.125");
        assert_eq!(jaccard_text(1, 1_000_000_000), "0.000000001");
        // No terminating decimal within nine digits: %.17g (expected
        // values from C printf).
        assert_eq!(jaccard_text(1, 3), "0.33333333333333331");
        assert_eq!(jaccard_text(2, 3), "0.66666666666666663");
        assert_eq!(jaccard_text(1, 7), "0.14285714285714285");
        assert_eq!(jaccard_text(1, 3_000_000_000), "3.3333333333333332e-10");
        assert_eq!(jaccard_text(1, 1 << 20), "9.5367431640625e-07");
        assert_eq!(jaccard_text(1, u64::MAX), "5.4210108624275222e-20");
    }

    #[test]
    fn format_g17_switches_notation_like_printf() {
        assert_eq!(format_g17(0.0001), "0.0001");
        assert_eq!(format_g17(0.00012), "0.00012");
        assert_eq!(format_g17(0.000099), "9.8999999999999994e-05");
        assert_eq!(format_g17(1.5), "1.5");
        assert_eq!(format_g17(1e17), "1e+17");
        assert_eq!(format_g17(12345678901234567.0), "12345678901234568");
    }

    #[test]
    fn invalid_mode_message_is_cut_like_upstream() {
        let long = vec![b'x'; 400];
        let msg = invalid_mode_message(&long);
        assert_eq!(msg.as_bytes().len(), 255);
        assert!(msg
            .as_bytes()
            .starts_with(b"ERR invalid mode argument: xxx"));
        // Raw bytes kept, cut at NUL, inner CR/LF as spaces, trailing
        // CR/LF trimmed (byte-for-byte with upstream's replies).
        assert_eq!(
            invalid_mode_message(b"\xffall\r\nx\0tail").as_bytes(),
            b"ERR invalid mode argument: \xffall  x"
        );
        assert_eq!(
            invalid_mode_message(b"abc\n").as_bytes(),
            b"ERR invalid mode argument: abc"
        );
        assert_eq!(
            invalid_mode_message(b"abc\r\n").as_bytes(),
            b"ERR invalid mode argument: abc"
        );
        assert_eq!(
            invalid_mode_message(b"\r").as_bytes(),
            b"ERR invalid mode argument: "
        );
        assert_eq!(
            invalid_mode_message(b"a\r\nb\n").as_bytes(),
            b"ERR invalid mode argument: a  b"
        );
    }

    #[test]
    fn value_reply_string_above_i64_max() {
        // Matches the C module's ReplyWithUint64: decimal bulk string.
        assert_eq!(
            value_reply::<RoaringTreemap>(i64::MAX as u64 + 1),
            ValkeyValue::BulkString("9223372036854775808".to_string())
        );
        assert_eq!(
            value_reply::<RoaringTreemap>(u64::MAX),
            ValkeyValue::BulkString("18446744073709551615".to_string())
        );
    }
}
