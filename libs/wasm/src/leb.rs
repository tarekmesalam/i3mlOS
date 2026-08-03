//! LEB128, the variable-length integer encoding WebAssembly is written in.
//!
//! Every decoder here is bounded: a malformed module must fail, never spin.
//! The reader is the first thing untrusted bytes touch, so it is also the
//! first place that must not be clever.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    /// Ran off the end of the input.
    Truncated,
    /// More continuation bytes than the target width allows.
    Overlong,
    /// The byte was not what the grammar expects here.
    Malformed,
}

pub type Result<T> = core::result::Result<T, Error>;

/// A cursor over module bytes. Bounds-checked on every read.
pub struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, position: 0 }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn seek(&mut self, position: usize) -> Result<()> {
        if position > self.bytes.len() {
            return Err(Error::Truncated);
        }
        self.position = position;
        Ok(())
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub fn at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    pub fn byte(&mut self) -> Result<u8> {
        let byte = *self.bytes.get(self.position).ok_or(Error::Truncated)?;
        self.position += 1;
        Ok(byte)
    }

    pub fn peek(&self) -> Result<u8> {
        self.bytes.get(self.position).copied().ok_or(Error::Truncated)
    }

    pub fn slice(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.position.checked_add(length).ok_or(Error::Truncated)?;
        let slice = self.bytes.get(self.position..end).ok_or(Error::Truncated)?;
        self.position = end;
        Ok(slice)
    }

    /// Unsigned LEB128, at most `bits` wide.
    pub fn unsigned(&mut self, bits: u32) -> Result<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.byte()?;
            let payload = (byte & 0x7f) as u64;
            if shift >= bits {
                // Only a zero payload can still fit at this point.
                if payload != 0 {
                    return Err(Error::Overlong);
                }
            } else {
                if shift + 7 > bits && payload >= (1 << (bits - shift)) {
                    return Err(Error::Overlong);
                }
                result |= payload << shift;
            }
            if byte & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= bits + 7 {
                return Err(Error::Overlong);
            }
        }
    }

    /// Signed LEB128, at most `bits` wide, sign-extended into i64.
    pub fn signed(&mut self, bits: u32) -> Result<i64> {
        let mut result: i64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.byte()?;
            // Guard the shift itself, not the loop afterwards: `<< 70` on an
            // i64 is an overflow panic in a checked build and undefined
            // nonsense in a release one, and twelve attacker-chosen bytes
            // were enough to reach it.
            if shift >= bits {
                return Err(Error::Overlong);
            }
            result |= ((byte & 0x7f) as i64) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && byte & 0x40 != 0 {
                    result |= -1i64 << shift;
                }
                if bits < 64 {
                    let limit = 1i64 << (bits - 1);
                    if result >= limit || result < -limit {
                        return Err(Error::Overlong);
                    }
                }
                return Ok(result);
            }
        }
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(self.unsigned(32)? as u32)
    }

    pub fn usize(&mut self) -> Result<usize> {
        Ok(self.unsigned(32)? as usize)
    }

    /// A UTF-8 name, as WebAssembly encodes it: length then bytes.
    pub fn name(&mut self) -> Result<&'a str> {
        let length = self.usize()?;
        let bytes = self.slice(length)?;
        core::str::from_utf8(bytes).map_err(|_| Error::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_values() {
        assert_eq!(Reader::new(&[0x00]).unsigned(32), Ok(0));
        assert_eq!(Reader::new(&[0x7f]).unsigned(32), Ok(127));
        assert_eq!(Reader::new(&[0x80, 0x01]).unsigned(32), Ok(128));
        assert_eq!(Reader::new(&[0xe5, 0x8e, 0x26]).unsigned(32), Ok(624_485));
    }

    #[test]
    fn signed_values() {
        assert_eq!(Reader::new(&[0x00]).signed(32), Ok(0));
        assert_eq!(Reader::new(&[0x7f]).signed(32), Ok(-1));
        assert_eq!(Reader::new(&[0x3f]).signed(32), Ok(63));
        assert_eq!(Reader::new(&[0x40]).signed(32), Ok(-64));
        assert_eq!(Reader::new(&[0xc0, 0xbb, 0x78]).signed(32), Ok(-123_456));
    }

    #[test]
    fn truncation_is_an_error_not_a_guess() {
        assert_eq!(Reader::new(&[0x80]).unsigned(32), Err(Error::Truncated));
        assert_eq!(Reader::new(&[]).byte(), Err(Error::Truncated));
        assert_eq!(Reader::new(&[0x02, b'h']).name(), Err(Error::Truncated));
    }

    #[test]
    fn an_overlong_i64_is_refused_rather_than_overflowing_a_shift() {
        // Eleven continuation bytes: the encoding the kernel's panic handler
        // used to meet. Ten is the most a 64-bit value can need.
        let bytes = [0x80; 11];
        assert_eq!(Reader::new(&bytes).signed(64), Err(Error::Overlong));
        assert_eq!(Reader::new(&[0xff; 12]).signed(64), Err(Error::Overlong));
        // The longest legal encoding still decodes.
        let mut legal = [0x80u8; 10];
        legal[9] = 0x7f;
        assert!(Reader::new(&legal).signed(64).is_ok());
    }

    #[test]
    fn overlong_encodings_are_refused() {
        // 0xff * 5 + 0x7f would decode past 32 bits.
        let bytes = [0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
        assert_eq!(Reader::new(&bytes).unsigned(32), Err(Error::Overlong));
    }
}
