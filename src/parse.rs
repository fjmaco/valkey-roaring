//! valkey-roaring: Argument parsing, byte-exact to the documented grammars.
//!
//! Upstream parses 32-bit values and bits through RedisModule_StringToLongLong
//! (Redis's string2ll: "0", or a non-zero digit followed by digits; no sign
//! for non-negative values, no leading zeros, no spaces) and 64-bit values
//! through its own StrToUInt64 (an optional '+', then digits, leading zeros
//! allowed). Bytes are parsed as they come: anything that is not ASCII
//! digits is not a number.

/// string2ll's grammar, limited to 0..=u32::MAX (upstream's StrToUInt32).
pub fn parse_u32_strict(bytes: &[u8]) -> Option<u32> {
    match bytes {
        b"0" => Some(0),
        [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit) && bytes.len() <= 10 => {
            let v: u64 = bytes
                .iter()
                .fold(0, |acc, &d| acc * 10 + u64::from(d - b'0'));
            u32::try_from(v).ok()
        }
        _ => None,
    }
}

/// Upstream's StrToUInt64: an optional '+' (not on its own), then digits,
/// leading zeros allowed, no overflow. That is exactly what
/// `str::parse::<u64>` accepts.
pub fn parse_u64_bytes(bytes: &[u8]) -> Option<u64> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Upstream's StrToBool: string2ll's grammar limited to 0 and 1, i.e.
/// exactly "0" or "1".
pub fn parse_bit_strict(bytes: &[u8]) -> Option<bool> {
    match bytes {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}

/// valkey-roaring 1.1.1's bit grammar, for commands replayed from its AOF
/// or replication stream: any 64-bit value (see parse_u64_bytes) equal to
/// 0 or 1 ("+1", "01" included).
pub fn parse_bit_legacy(bytes: &[u8]) -> Option<bool> {
    match parse_u64_bytes(bytes)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// An argument as upstream's strcmp sees it: up to the first NUL byte.
pub fn c_str(bytes: &[u8]) -> &[u8] {
    bytes.split(|&b| b == 0).next().unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_grammar_matches_string2ll() {
        let ok: [(&[u8], u32); 5] = [
            (b"0", 0),
            (b"7", 7),
            (b"10", 10),
            (b"4294967295", u32::MAX),
            (b"1000000000", 1_000_000_000),
        ];
        for (bytes, v) in ok {
            assert_eq!(parse_u32_strict(bytes), Some(v), "{:?}", bytes);
        }
        let bad: [&[u8]; 18] = [
            b"",
            b"+5",
            b"005",
            b"00",
            b"-0",
            b"-1",
            b" 5",
            b"5 ",
            b"1e3",
            b"0x10",
            b"1.0",
            b"4294967296",
            b"18446744073709551615",
            b"99999999999",
            b"\xff1",
            b"1\x00",
            b"\xef\xbc\x91",
            b"+",
        ];
        for bytes in bad {
            assert_eq!(parse_u32_strict(bytes), None, "{:?}", bytes);
        }
    }

    /// The 64-bit parser must accept and reject exactly what upstream's
    /// StrToUInt64 does (a reimplementation of it, below).
    #[test]
    fn u64_grammar_matches_upstream_str_to_uint64() {
        fn upstream(s: &[u8]) -> Option<u64> {
            if s.is_empty() || s[0] == b'-' || s == b"+" {
                return None;
            }
            let digits = s.strip_prefix(b"+").unwrap_or(s);
            let mut v: u64 = 0;
            for &c in digits {
                if !c.is_ascii_digit() {
                    return None;
                }
                v = v.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
            }
            Some(v)
        }
        let cases: [&[u8]; 22] = [
            b"0",
            b"7",
            b"+7",
            b"007",
            b"18446744073709551615",
            b"18446744073709551616",
            b"-0",
            b"-1",
            b"+",
            b"",
            b" 1",
            b"1 ",
            b"1e3",
            b"0x10",
            b"1.0",
            b"++1",
            b"\xff1",
            b"1\x00",
            b"\xef\xbc\x91",
            b"+0",
            b"99999999999999999999999",
            b"4294967296",
        ];
        for bytes in cases {
            assert_eq!(parse_u64_bytes(bytes), upstream(bytes), "input {:?}", bytes);
        }
    }

    #[test]
    fn bit_grammars() {
        assert_eq!(parse_bit_strict(b"0"), Some(false));
        assert_eq!(parse_bit_strict(b"1"), Some(true));
        for bad in [&b"01"[..], b"+1", b"-0", b"2", b"", b" 1"] {
            assert_eq!(parse_bit_strict(bad), None, "{bad:?}");
        }
        // 1.1.1 parsed bits as u64 then required 0 or 1.
        for (ok, v) in [
            (&b"01"[..], true),
            (b"+1", true),
            (b"000", false),
            (b"1", true),
        ] {
            assert_eq!(parse_bit_legacy(ok), Some(v), "{ok:?}");
        }
        for bad in [&b"2"[..], b"-0", b"", b" 1", b"1 "] {
            assert_eq!(parse_bit_legacy(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn c_str_stops_at_nul() {
        assert_eq!(c_str(b"COUNT"), b"COUNT");
        assert_eq!(c_str(b"COUNT\0junk"), b"COUNT");
        assert_eq!(c_str(b"\0"), b"");
        assert_eq!(c_str(b""), b"");
    }
}
