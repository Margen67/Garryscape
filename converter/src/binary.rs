// SPDX-License-Identifier: AGPL-3.0-or-later
use anyhow::{ensure, Context as _, Result};
/// Bounds-checked cursor over a big-endian cache stream.
pub(crate) struct Reader<'data> {
    /// Original cache payload; never modified.
    pub data: &'data [u8],
    /// Byte offset of the next unread value.
    pub position: usize,
}
impl<'data> Reader<'data> {
    /// Start reading at the beginning of a cache payload.
    pub(crate) const fn new(data: &'data [u8]) -> Self {
        Self { data, position: 0 }
    }
    /// Consume exactly `count` bytes, rejecting truncated input.
    pub(crate) fn take(&mut self, count: usize) -> Result<&'data [u8]> {
        let end = self
            .position
            .checked_add(count)
            .context("Stream offset overflow")?;
        let bytes = self
            .data
            .get(self.position..end)
            .with_context(|| format!("Truncated stream at {} (+{count})", self.position))?;
        self.position = end;
        Ok(bytes)
    }
    /// Read one unsigned byte.
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(*self.take(1)?.first().context("Missing byte")?)
    }
    /// Read an unsigned big-endian 16-bit value.
    #[allow(
        clippy::big_endian_bytes,
        reason = "the JS5 cache format specifies big-endian byte order"
    )]
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    /// Read an unsigned big-endian 24-bit value.
    pub(crate) fn u24(&mut self) -> Result<u32> {
        let [high, middle, low]: [u8; 3] = self.take(3)?.try_into()?;
        Ok((u32::from(high) << 16) | (u32::from(middle) << 8) | u32::from(low))
    }
    /// Read an unsigned big-endian 32-bit value.
    #[allow(
        clippy::big_endian_bytes,
        reason = "the JS5 cache format specifies big-endian byte order"
    )]
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    /// Read a signed big-endian 32-bit value.
    #[allow(
        clippy::big_endian_bytes,
        reason = "the JS5 cache format specifies big-endian byte order"
    )]
    pub(crate) fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    /// Read a version-dependent JS5 identifier.
    pub(crate) fn id(&mut self, protocol: u8) -> Result<u32> {
        if protocol >= 7
            && self
                .data
                .get(self.position)
                .is_some_and(|value| *value >= 128)
        {
            Ok(self.u32()? & 0x7fff_ffff)
        } else {
            Ok(u32::from(self.u16()?))
        }
    }
    /// Reject any bytes left over after decoding a complete payload.
    pub(crate) fn finish(&self) -> Result<()> {
        ensure!(
            self.position == self.data.len(),
            "Unconsumed stream data at {} of {} bytes",
            self.position,
            self.data.len()
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests panic on unexpected decode failures"
)]
mod tests {
    use super::Reader;

    #[test]
    fn failed_reads_preserve_the_cursor() {
        let mut reader = Reader::new(&[0x12, 0x34, 0x56]);
        assert_eq!(reader.u8().unwrap(), 0x12);
        let _truncated = reader.u24().unwrap_err();
        assert_eq!(reader.position, 1);
        let _overflow = reader.take(usize::MAX).unwrap_err();
        assert_eq!(reader.position, 1);
        assert_eq!(reader.u16().unwrap(), 0x3456);
        reader.finish().unwrap();
    }

    #[test]
    fn signed_values_keep_their_cache_bit_pattern() {
        let mut reader = Reader::new(&[0xff, 0xff, 0xff, 0xfe]);
        assert_eq!(reader.i32().unwrap(), -2_i32);
        reader.finish().unwrap();
        let _past_end = reader.u8().unwrap_err();
        assert_eq!(reader.position, 4);
    }
}
