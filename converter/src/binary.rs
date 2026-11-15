use anyhow::{bail, ensure, Result};
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub position: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(
            n <= self.data.len().saturating_sub(self.position),
            "Truncated stream at {} (+{n})",
            self.position
        );
        let start = self.position;
        self.position += n;
        Ok(&self.data[start..self.position])
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    pub fn u24(&mut self) -> Result<u32> {
        let p = self.take(3)?;
        Ok((p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn id(&mut self, protocol: u8) -> Result<u32> {
        if protocol >= 7 && self.data.get(self.position).is_some_and(|v| *v >= 128) {
            Ok(self.u32()? & 0x7fff_ffff)
        } else {
            Ok(self.u16()? as u32)
        }
    }
    pub fn finish(&self) -> Result<()> {
        if self.position != self.data.len() {
            bail!("Trailing data: {} bytes", self.data.len() - self.position);
        }
        Ok(())
    }
}
