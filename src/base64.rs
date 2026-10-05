//! valkey-roaring: Base64 (RFC 4648, standard alphabet, with padding) for
//! the text form of EXPORT and IMPORT blobs.
//!
//! Decoding is strict, so a blob has exactly one text form: the length is a
//! multiple of 4, `=` appears only as one or two final padding characters,
//! the bits the padding leaves unused are zero, and nothing outside the
//! alphabet is accepted (no whitespace, no line breaks, no URL-safe `-`/`_`).

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Marks a byte outside the alphabet in `DECODE`.
const INVALID: u8 = 0xFF;

/// Each byte's 6-bit value, or INVALID.
const DECODE: [u8; 256] = {
    let mut table = [INVALID; 256];
    let mut i = 0;
    while i < 64 {
        table[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// Base64 text of `bytes`.
pub fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    let (chunks, remainder) = bytes.as_chunks::<3>();
    for &[a, b, c] in chunks {
        let n = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        out.extend_from_slice(&[
            ALPHABET[(n >> 18) as usize & 63],
            ALPHABET[(n >> 12) as usize & 63],
            ALPHABET[(n >> 6) as usize & 63],
            ALPHABET[n as usize & 63],
        ]);
    }
    match *remainder {
        [a] => {
            let n = u32::from(a) << 16;
            out.extend_from_slice(&[
                ALPHABET[(n >> 18) as usize & 63],
                ALPHABET[(n >> 12) as usize & 63],
                b'=',
                b'=',
            ]);
        }
        [a, b] => {
            let n = u32::from(a) << 16 | u32::from(b) << 8;
            out.extend_from_slice(&[
                ALPHABET[(n >> 18) as usize & 63],
                ALPHABET[(n >> 12) as usize & 63],
                ALPHABET[(n >> 6) as usize & 63],
                b'=',
            ]);
        }
        _ => {}
    }
    out
}

/// The bytes `text` encodes, or None if it is not strict Base64 (see the
/// module documentation).
pub fn decode(text: &[u8]) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let Some((last, body)) = text.as_chunks::<4>().0.split_last() else {
        return Some(out);
    };
    for quad in body {
        let n = sextets(quad)?;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    // The final quad may end in padding ('A' decodes to zero bits): one
    // byte leaves the low 4 bits of the second character unused, two bytes
    // the low 2 bits of the third, and those bits must be zero.
    let (n, bytes, unused) = match *last {
        [a, b, b'=', b'='] => (sextets(&[a, b, b'A', b'A'])?, 1, 0xFFFF),
        [a, b, c, b'='] => (sextets(&[a, b, c, b'A'])?, 2, 0xFF),
        quad => (sextets(&quad)?, 3, 0),
    };
    if n & unused != 0 {
        return None;
    }
    out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8][..bytes]);
    Some(out)
}

/// Four alphabet characters as 24 bits, or None if any is outside the
/// alphabet (`=` included).
fn sextets(quad: &[u8; 4]) -> Option<u32> {
    let [a, b, c, d] = quad.map(|ch| DECODE[usize::from(ch)]);
    // Valid values fit 6 bits; INVALID does not.
    if (a | b | c | d) > 63 {
        return None;
    }
    Some(u32::from(a) << 18 | u32::from(b) << 12 | u32::from(c) << 6 | u32::from(d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::xorshift;

    /// An independent bit-by-bit reference encoder.
    fn reference_encode(bytes: &[u8]) -> Vec<u8> {
        let mut bits = Vec::new();
        for b in bytes {
            for i in (0..8).rev() {
                bits.push((b >> i) & 1);
            }
        }
        while !bits.len().is_multiple_of(6) {
            bits.push(0);
        }
        let mut out: Vec<u8> = bits
            .chunks(6)
            .map(|six| ALPHABET[six.iter().fold(0usize, |acc, &bit| acc << 1 | bit as usize)])
            .collect();
        while !out.len().is_multiple_of(4) {
            out.push(b'=');
        }
        out
    }

    #[test]
    fn rfc4648_test_vectors() {
        let vectors: [(&[u8], &[u8]); 7] = [
            (b"", b""),
            (b"f", b"Zg=="),
            (b"fo", b"Zm8="),
            (b"foo", b"Zm9v"),
            (b"foob", b"Zm9vYg=="),
            (b"fooba", b"Zm9vYmE="),
            (b"foobar", b"Zm9vYmFy"),
        ];
        for (plain, text) in vectors {
            assert_eq!(encode(plain), text, "{plain:?}");
            assert_eq!(decode(text).as_deref(), Some(plain), "{text:?}");
        }
        // Every byte value and both padding lengths.
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)).unwrap(), all);
        assert_eq!(encode(&[0xFF, 0xEF]), b"/+8=");
    }

    #[test]
    fn matches_reference_and_round_trips() {
        let mut state = 0x0BA5_E640_0BA5_E640u64;
        for len in 0..300 {
            let bytes: Vec<u8> = (0..len).map(|_| xorshift(&mut state) as u8).collect();
            let text = encode(&bytes);
            assert_eq!(text, reference_encode(&bytes), "length {len}");
            assert_eq!(decode(&text).as_deref(), Some(&bytes[..]), "length {len}");
        }
    }

    #[test]
    fn rejects_non_strict_text() {
        let bad: [&[u8]; 22] = [
            b"Z",         // length not a multiple of 4
            b"Zg=",       //
            b"Zg",        // padding left out
            b"Zm9vY",     //
            b"Zg===",     //
            b"====",      // padding only
            b"Z===",      // three padding characters
            b"=Zg=",      // padding first
            b"Zg==Zg==",  // padding inside the text
            b"Zm=v",      // padding before a data character
            b"Zh==",      // nonzero bits under the padding
            b"Zm9=",      //
            b"Zm8 ",      // whitespace
            b"Zm8\n",     //
            b" Zm8=",     //
            b"Zm9v\r\n",  //
            b"Zm-v",      // URL-safe alphabet
            b"Zm_v",      //
            b"Zm9\0",     // NUL
            b"Zm9\xff",   // non-ASCII
            b"Zm9v.A==",  // punctuation
            b"Zm9vYmFy=", // overlong
        ];
        for text in bad {
            assert_eq!(decode(text), None, "{:?}", String::from_utf8_lossy(text));
        }
    }

    #[test]
    fn decoding_is_canonical() {
        // Whatever decodes re-encodes to the same text: one text per blob,
        // so the text of a canonical EXPORT is canonical too. Random texts
        // over the alphabet, padding and a few strays, most of them invalid.
        const CHARS: &[u8] = b"ABCDQRSTghijwxyz0189+/=== \n-_\0";
        let mut state = 0xD1CE_D1CE_D1CE_D1CEu64;
        let mut decoded = 0;
        for _ in 0..50_000 {
            let len = (xorshift(&mut state) % 13) as usize;
            let text: Vec<u8> = (0..len)
                .map(|_| CHARS[(xorshift(&mut state) % CHARS.len() as u64) as usize])
                .collect();
            if let Some(bytes) = decode(&text) {
                assert_eq!(encode(&bytes), text, "{:?}", String::from_utf8_lossy(&text));
                decoded += 1;
            }
        }
        assert!(decoded > 1_000, "only {decoded} valid texts generated");
    }

    #[test]
    fn every_padding_bit_pattern_is_checked() {
        // One input byte: the second character carries 2 data bits and 4
        // padding bits; only the 4 characters with zero padding bits decode.
        for (i, &ch) in ALPHABET.iter().enumerate() {
            let text = [b'Q', ch, b'=', b'='];
            assert_eq!(decode(&text).is_some(), i % 16 == 0, "{}", ch as char);
            // Two input bytes: the third character carries 2 padding bits.
            let text = [b'Q', b'U', ch, b'='];
            assert_eq!(decode(&text).is_some(), i % 4 == 0, "{}", ch as char);
        }
    }
}
