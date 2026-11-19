//! Modified UTF-8, the encoding of `CONSTANT_Utf8` payloads.
//!
//! Class files do not store standard UTF-8. Two differences matter:
//!
//! - NUL is encoded as `C0 80`, so the payload never contains a zero byte.
//! - Characters outside the BMP are stored as two three-byte surrogate halves
//!   (CESU-8 style) rather than as a four-byte sequence.
//!
//! `str::from_utf8` rejects both forms, which is why this decoder exists instead of a
//! convenience call.
//!
//! One more difference is forced by Rust rather than by the format: a class file may store a
//! *lone* surrogate (JVMS §4.4.7 allows any U+0800..U+FFFF in three bytes, and
//! `sun.nio.cs.GB18030` does), while a `String` cannot hold one. Such a character decodes to
//! U+FFFD — lossy, but names are never surrogates and only string constants are affected.

use crate::error::Error;

/// Decodes one `CONSTANT_Utf8` payload.
///
/// `index` is the constant pool index, carried only so the error names the entry.
pub fn decode(bytes: &[u8], index: u16) -> Result<String, Error> {
    let bad = |offset: usize| Error::BadModifiedUtf8 { index, offset };

    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        let b0 = bytes[i];
        if b0 < 0x80 {
            out.push(b0 as char);
            i += 1;
            continue;
        }

        if b0 & 0xE0 == 0xC0 {
            let b1 = *bytes.get(i + 1).ok_or_else(|| bad(i))?;
            if b1 & 0xC0 != 0x80 {
                return Err(bad(i));
            }
            let cp = ((u32::from(b0 & 0x1F)) << 6) | u32::from(b1 & 0x3F);
            out.push(char::from_u32(cp).ok_or_else(|| bad(i))?);
            i += 2;
            continue;
        }

        if b0 & 0xF0 == 0xE0 {
            let b1 = *bytes.get(i + 1).ok_or_else(|| bad(i))?;
            let b2 = *bytes.get(i + 2).ok_or_else(|| bad(i))?;
            if b1 & 0xC0 != 0x80 || b2 & 0xC0 != 0x80 {
                return Err(bad(i));
            }
            let cp = ((u32::from(b0 & 0x0F)) << 12)
                | ((u32::from(b1 & 0x3F)) << 6)
                | u32::from(b2 & 0x3F);

            if (0xD800..=0xDBFF).contains(&cp) {
                // High surrogate: if a low surrogate follows, the pair is one supplementary
                // code point. A lone surrogate is legal in a class file (JVMS §4.4.7 allows
                // any U+0800..U+FFFF in three bytes) but not representable in a Rust `String`,
                // so it becomes U+FFFD. Names never contain one; only string constants do.
                let low = bytes
                    .get(i + 3..i + 6)
                    .filter(|rest| rest[0] & 0xF0 == 0xE0)
                    .map(|rest| {
                        ((u32::from(rest[0] & 0x0F)) << 12)
                            | ((u32::from(rest[1] & 0x3F)) << 6)
                            | u32::from(rest[2] & 0x3F)
                    });
                match low {
                    Some(low) if (0xDC00..=0xDFFF).contains(&low) => {
                        let combined = 0x1_0000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                        out.push(char::from_u32(combined).ok_or_else(|| bad(i))?);
                        i += 6;
                    }
                    _ => {
                        out.push(char::REPLACEMENT_CHARACTER);
                        i += 3;
                    }
                }
                continue;
            }

            if (0xDC00..=0xDFFF).contains(&cp) {
                // A lone low surrogate: legal in the class file, not in a Rust `String`.
                out.push(char::REPLACEMENT_CHARACTER);
                i += 3;
                continue;
            }

            out.push(char::from_u32(cp).ok_or_else(|| bad(i))?);
            i += 3;
            continue;
        }

        return Err(bad(i));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_ascii() {
        assert_eq!(decode(b"java/lang/Object", 1).unwrap(), "java/lang/Object");
    }

    #[test]
    fn decodes_encoded_nul() {
        // NUL is stored as C0 80, never as a zero byte.
        assert_eq!(decode(b"a\xC0\x80b", 7).unwrap(), "a\0b");
    }

    #[test]
    fn decodes_surrogate_pair_as_one_code_point() {
        // U+1F600 stored as D83D DE00 in CESU-8 style.
        let bytes = [0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
        assert_eq!(decode(&bytes, 3).unwrap(), "\u{1F600}");
    }

    #[test]
    fn decodes_a_lone_surrogate_lossily() {
        // `sun.nio.cs.GB18030` carries mapping strings with unpaired surrogates. JVMS §4.4.7
        // allows them; a Rust `String` cannot hold one, so it becomes U+FFFD rather than
        // failing the whole class file.
        let high = [0xED, 0xA0, 0xBD];
        assert_eq!(decode(&high, 4).unwrap(), "\u{FFFD}");
        let low = [0xED, 0xBA, 0x9A];
        assert_eq!(decode(&low, 5).unwrap(), "\u{FFFD}");
        // A high surrogate followed by something that is not a low surrogate is lossy too.
        let broken = [0xED, 0xA0, 0xBD, 0x41];
        assert_eq!(decode(&broken, 6).unwrap(), "\u{FFFD}A");
    }

    #[test]
    fn rejects_a_plain_four_byte_sequence() {
        // Standard UTF-8 would use F0 9F 98 80; class files must not.
        let bytes = [0xF0, 0x9F, 0x98, 0x80];
        assert!(decode(&bytes, 5).is_err());
    }
}
