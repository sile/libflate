use std::io;
use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;

#[derive(Debug)]
pub struct BitWriter<W> {
    inner: W,
    buf: u32,
    end: u8,
}
impl<W> BitWriter<W>
    where W: io::Write
{
    pub fn new(inner: W) -> Self {
        BitWriter {
            inner: inner,
            buf: 0,
            end: 0,
        }
    }
    pub fn as_inner_ref(&self) -> &W {
        &self.inner
    }
    pub fn as_inner_mut(&mut self) -> &mut W {
        &mut self.inner
    }
    pub fn into_inner(self) -> W {
        self.inner
    }
    pub fn flush(&mut self) -> io::Result<()> {
        while self.end > 0 {
            try!(self.inner.write_u8(self.buf as u8));
            self.buf >>= 8;
            self.end = self.end.saturating_sub(8);
        }
        Ok(())
    }
    pub fn write_bit(&mut self, bit: bool) -> io::Result<()> {
        debug_assert!(self.end + 1 <= 32);
        self.buf |= (bit as u32) << self.end;
        self.end += 1;
        self.flush_if_needed()
    }
    pub fn write_bits(&mut self, bitwidth: u8, bits: u16) -> io::Result<()> {
        debug_assert!(bitwidth < 16);
        debug_assert!(self.end + bitwidth <= 32);
        self.buf |= (bits as u32) << self.end;
        self.end += bitwidth;
        self.flush_if_needed()
    }
    fn flush_if_needed(&mut self) -> io::Result<()> {
        if self.end >= 16 {
            try!(self.inner.write_u16::<LittleEndian>(self.buf as u16));
            self.end -= 16;
            self.buf >>= 16;
        }
        Ok(())
    }
}

const LENGTH: u8 = 32;

#[derive(Debug)]
pub struct BitReader<R> {
    inner: R,
    last_read: u32,
    offset: u8,
}
impl<R> BitReader<R>
    where R: io::Read
{
    pub fn new(inner: R) -> Self {
        BitReader {
            inner: inner,
            last_read: 0,
            offset: LENGTH,
        }
    }
    pub fn read_bit(&mut self) -> io::Result<bool> {
        if self.offset == LENGTH {
            try!(self.fill_next_u8());
        }
        let bit = (self.last_read & (1 << self.offset)) != 0;
        self.offset += 1;
        Ok(bit)
    }
    #[inline]
    pub fn skip_bits(&mut self, bitwidth: u8) {
        debug_assert!(LENGTH - self.offset >= bitwidth);
        self.offset += bitwidth;
    }
    #[inline]
    pub fn peek_bits(&mut self, bitwidth: u8) -> io::Result<u16> {
        debug_assert!(bitwidth <= 16);
        while (LENGTH - self.offset) < bitwidth {
            try!(self.fill_next_u8());
        }
        let bits = (self.last_read >> self.offset) as u16;
        Ok(bits & ((1 << bitwidth) - 1))
    }
    pub fn read_bits(&mut self, bitwidth: u8) -> io::Result<u16> {
        let x = try!(self.peek_bits(bitwidth));
        self.skip_bits(bitwidth);
        Ok(x)
    }
    pub fn reset(&mut self) {
        self.offset = LENGTH;
    }
    pub fn as_inner_ref(&self) -> &R {
        &self.inner
    }
    pub fn as_inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }
    pub fn into_inner(self) -> R {
        self.inner
    }
    #[inline]
    fn fill_next_u8(&mut self) -> io::Result<()> {
        self.offset -= 8;
        self.last_read >>= 8;

        let next = try!(self.inner.read_u8()) as u32;
        self.last_read |= next << (LENGTH - 8);
        Ok(())
    }
}
