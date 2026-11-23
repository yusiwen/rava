//! Bounds-checked big-endian cursor over class file bytes.

use crate::error::Error;

pub struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.offset.checked_add(n).ok_or(Error::UnexpectedEof {
            offset: self.offset,
            needed: n,
        })?;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof {
                offset: self.offset,
                needed: n,
            });
        }
        let slice = &self.data[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    pub fn u1(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    pub fn u2(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u4(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u8(&mut self) -> Result<u64, Error> {
        let b = self.take(8)?;
        Ok(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Reads `n` bytes without copying. Callers that need owned data copy explicitly.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        self.take(n)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), Error> {
        self.take(n).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_big_endian_values() {
        let mut r = Reader::new(&[0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x34]);
        assert_eq!(r.u4().unwrap(), 0xCAFE_BABE);
        assert_eq!(r.u2().unwrap(), 52);
        assert!(r.is_empty());
    }

    #[test]
    fn refuses_to_read_past_the_end() {
        let mut r = Reader::new(&[0x00]);
        let err = r.u2().unwrap_err();
        assert_eq!(
            err,
            Error::UnexpectedEof {
                offset: 0,
                needed: 2
            }
        );
        assert_eq!(r.offset(), 0, "a failed read must not advance the cursor");
    }
}
