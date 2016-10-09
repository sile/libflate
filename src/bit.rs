use std::io;
use byteorder::ReadBytesExt;

const LENGTH: u8 = 32;

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
    pub fn skip_bits(&mut self, bitwidth: u8) {
        debug_assert!(LENGTH - self.offset >= bitwidth);
        self.offset += bitwidth;
    }
    pub fn peek_bits(&mut self, min_bitwidth: u8) -> io::Result<u16> {
        debug_assert!(min_bitwidth <= 16);
        while (LENGTH - self.offset) < min_bitwidth {
            try!(self.fill_next_u8());
        }
        let bits = self.last_read >> self.offset;
        Ok(bits as u16)
    }
    pub fn read_exact_bits(&mut self, bitwidth: u8) -> io::Result<u16> {
        let x = try!(self.peek_bits(bitwidth));
        self.skip_bits(bitwidth);
        Ok(x & ((1 << bitwidth) - 1))
    }
    pub fn read_u16(&mut self) -> io::Result<u16> {
        self.read_exact_bits(16)
    }
    pub fn align_u8(&mut self) {
        if self.offset % 8 != 0 {
            let delta = 8 - self.offset % 8;
            self.offset += delta;
        }
    }
    pub fn as_inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }
    pub fn into_byte_reader(self) -> R {
        self.inner
    }
    fn fill_next_u8(&mut self) -> io::Result<()> {
        self.offset -= 8;
        self.last_read >>= 8;

        let next = try!(self.inner.read_u8()) as u32;
        self.last_read |= next << (LENGTH - 8);
        Ok(())
    }
}
