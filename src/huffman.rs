/// Length-limited Huffman Codes
///
/// Reference: https://www.ics.uci.edu/~dan/pubs/LenLimHuff.pdf
use std::io;

use bit::BitReader;

pub struct DecoderBuilder {
    table: Vec<u16>,
    max_bitwidth: u8,
}
impl DecoderBuilder {
    pub fn new(max_bitwidth: u8) -> Self {
        debug_assert!(max_bitwidth <= 15);
        DecoderBuilder {
            table: vec![0; 1 << max_bitwidth],
            max_bitwidth: max_bitwidth,
        }
    }
    pub fn set_mapping(&mut self, bitwidth: u8, from: u16, to: u16) {
        debug_assert!(bitwidth > 0);
        debug_assert!(bitwidth <= self.max_bitwidth);

        // Converts from little-endian to big-endian
        let mut from_le = from;
        let mut from_be = 0;
        for _ in 0..bitwidth {
            from_be <<= 1;
            from_be |= from_le & 1;
            from_le >>= 1;
        }

        // `bitwidth` encoded `to` value
        let value = (to << 4) | bitwidth as u16;

        // Sets the mapping to all possible indices
        for padding in 0..(1 << (self.max_bitwidth - bitwidth)) {
            let i = ((padding << bitwidth) | from_be) as usize;
            debug_assert_eq!(self.table[i], 0);
            unsafe {
                *self.table.get_unchecked_mut(i) = value;
            }
        }
    }
    pub fn finish(self) -> Decoder {
        Decoder {
            table: self.table,
            max_bitwidth: self.max_bitwidth,
        }
    }
    pub fn from_bitwidthes(bitwidthes: &[u8]) -> Decoder {
        debug_assert!(bitwidthes.len() > 0);

        // NOTE: Canonical Huffman Code
        let mut codes = Vec::new();
        for (code, count) in bitwidthes.iter().cloned().enumerate() {
            if count == 0 {
                continue;
            }
            codes.push((code as u16, count));
        }
        codes.sort_by_key(|x| x.1);

        let mut builder = Self::new(codes.last().unwrap().1);
        let mut from = 0;
        let mut prev_count = 0;
        for (code, count) in codes {
            if prev_count != count {
                from <<= count - prev_count;
                prev_count = count;
            }
            builder.set_mapping(count, from, code);
            from += 1;
        }
        builder.finish()
    }
}

pub struct Decoder {
    table: Vec<u16>,
    max_bitwidth: u8,
}
impl Decoder {
    #[inline]
    pub fn decode<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<u16>
        where R: io::Read
    {
        let code = try!(reader.peek_bits(self.max_bitwidth));
        let value = unsafe { *self.table.get_unchecked(code as usize) };
        if value == 0 {
            Err(invalid_data_error!("Invalid huffman coded stream"))
        } else {
            let bitwidth = value & 0b1111;
            let decoded = value >> 4;
            reader.skip_bits(bitwidth as u8);
            Ok(decoded)
        }
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn it_works() {}
}
