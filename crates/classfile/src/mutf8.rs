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
                // High surrogate: a low surrogate must follow, and the pair encodes
                // one supplementary code point.
                let b3 = *bytes.get(i + 3).ok_or_else(|| bad(i))?;
                let b4 = *bytes.get(i + 4).ok_or_else(|| bad(i))?;
                let b5 = *bytes.get(i + 5).ok_or_else(|| bad(i))?;
                if b3 & 0xF0 != 0xE0 || b4 & 0xC0 != 0x80 || b5 & 0xC0 != 0x80 {
                    return Err(bad(i));
                }
                let low = ((u32::from(b3 & 0x0F)) << 12)
                    | ((u32::from(b4 & 0x3F)) << 6)
                    | u32::from(b5 & 0x3F);
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(bad(i));
                }
                let combined = 0x1_0000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                out.push(char::from_u32(combined).ok_or_else(|| bad(i))?);
                i += 6;
                continue;
            }

            if (0xDC00..=0xDFFF).contains(&cp) {
                // A low surrogate with no preceding high surrogate.
                return Err(bad(i));
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
    fn rejects_a_lone_surrogate() {
        let bytes = [0xED, 0xA0, 0xBD];
        assert!(decode(&bytes, 4).is_err());
    }

    #[test]
    fn rejects_a_plain_four_byte_sequence() {
        // Standard UTF-8 would use F0 9F 98 80; class files must not.
        let bytes = [0xF0, 0x9F, 0x98, 0x80];
        assert!(decode(&bytes, 5).is_err());
    }
}
