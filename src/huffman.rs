/// Length-limited Huffman Codes
///
/// Reference: https://www.ics.uci.edu/~dan/pubs/LenLimHuff.pdf
use std::io;

use bit::BitReader;

// TODO: rename (DecodeXXX)
pub struct Codes {
    table: Vec<u16>,
    max_bitwidth: u8,
}
impl Codes {
    pub fn new(max_bitwidth: u8) -> Self {
        debug_assert!(max_bitwidth <= 15);
        Codes {
            table: vec![0; 1 << max_bitwidth],
            max_bitwidth: max_bitwidth,
        }
    }
    pub fn max_bitwidth(&self) -> u8 {
        self.max_bitwidth
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
    pub fn decode(&self, code: u16) -> Option<(u8, u16)> {
        let i = code & ((1 << self.max_bitwidth) - 1);
        let value = unsafe { *self.table.get_unchecked(i as usize) };
        if value == 0 {
            None
        } else {
            let bitwidth = value & 0b1111;
            let decoded = value >> 4;
            Some((bitwidth as u8, decoded))
        }
    }
}

// TODO: use lazy_static
pub fn fixed_literal_length_codes() -> Codes {
    let mut codes = Codes::new(9);
    for i in 0..144 {
        codes.set_mapping(8, 0b0011_0000 + i, i);
    }
    for i in 144..256 {
        codes.set_mapping(9, 0b1_1001_0000 + i - 144, i);
    }
    for i in 256..280 {
        codes.set_mapping(7, 0b000_0000 + i - 256, i);
    }
    for i in 280..287 {
        codes.set_mapping(8, 0b1100_0000 + i - 280, i);
    }
    codes
}

pub fn fixed_distance_codes() -> Codes {
    let mut codes = Codes::new(5);
    for i in 0..30 {
        codes.set_mapping(5, i, i);
    }
    codes
}

pub struct Decoder2 {
    codes: Codes,
}
impl Decoder2 {
    pub fn from_lens(lens: &[u8]) -> Self {
        // NOTE: Canonical Huffman Code
        let mut codes = Vec::new();
        for (code, count) in lens.iter().cloned().enumerate() {
            if count == 0 {
                continue;
            }
            codes.push((code as u16, count));
        }
        // println!("=> {:?}", codes);
        codes.sort_by_key(|x| x.1);

        let mut cs = Codes::new(codes.last().unwrap().1);
        let mut from = 0;
        let mut prev_count = 0;
        for (code, count) in codes {
            if prev_count != count {
                from <<= count - prev_count;
                prev_count = count;
            }
            cs.set_mapping(count, from, code);
            from += 1;
        }
        Decoder2 { codes: cs }
    }
    pub fn decode<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<u16>
        where R: io::Read
    {
        let code = try!(reader.peek_bits(self.codes.max_bitwidth()));
        if let Some((bitwidth, decoded)) = self.codes.decode(code) {
            reader.skip_bits(bitwidth);
            Ok(decoded)
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid huffman coded stream"))
        }
    }
    pub fn codes(self) -> Codes {
        self.codes
    }
}

pub struct Decoder {
    literal_codes: Codes,
    distance_codes: Codes,
}
impl Decoder {
    pub fn new(lite: Codes, dist: Codes) -> Self {
        Decoder {
            literal_codes: lite,
            distance_codes: dist,
        }
    }
    pub fn new_fixed() -> Self {
        Decoder {
            literal_codes: fixed_literal_length_codes(),
            distance_codes: fixed_distance_codes(),
        }
    }
    fn decode_literal_or_length<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<Symbol>
        where R: io::Read
    {
        let code = try!(reader.peek_bits(self.literal_codes.max_bitwidth()));
        if let Some((bitwidth, decoded)) = self.literal_codes.decode(code) {
            reader.skip_bits(bitwidth);
            let s = match decoded {
                0...255 => Symbol::Literal(decoded as u8),
                256 => Symbol::EndOfBlock,
                length_code => {
                    let (base, extra_bits) = decode_length(length_code);
                    let extra = try!(reader.read_exact_bits(extra_bits));
                    let length = base + extra;
                    Symbol::Share {
                        length: length,
                        distance: 0,
                    }
                }
            };
            Ok(s)
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData,
                               "Can not decode literal or length code"))
        }
    }
    fn decode_distance<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<u16>
        where R: io::Read
    {
        let code = try!(reader.peek_bits(self.distance_codes.max_bitwidth()));
        if let Some((bitwidth, decoded)) = self.distance_codes.decode(code) {
            reader.skip_bits(bitwidth);
            let (base, extra_bits) = decode_distance(decoded);
            let extra = try!(reader.read_exact_bits(extra_bits));
            let distance = base + extra;
            Ok(distance)
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "Can not decode distance code"))
        }
    }
    pub fn decode_one<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<Symbol>
        where R: io::Read
    {
        self.decode_literal_or_length(reader).and_then(|mut s| {
            if let Symbol::Share { ref mut distance, .. } = s {
                *distance = try!(self.decode_distance(reader));
            }
            Ok(s)
        })
    }
}

fn decode_distance(code: u16) -> (u16, u8) {
    let table = [(1, 0),
                 (2, 0),
                 (3, 0),
                 (4, 0),
                 (5, 1),
                 (7, 1),
                 (9, 2),
                 (13, 2),
                 (17, 3),
                 (25, 3),
                 (33, 4),
                 (49, 4),
                 (65, 5),
                 (97, 5),
                 (129, 6),
                 (193, 6),
                 (257, 7),
                 (385, 7),
                 (513, 8),
                 (769, 8),
                 (1025, 9),
                 (1537, 9),
                 (2049, 10),
                 (3073, 10),
                 (4097, 11),
                 (6145, 11),
                 (8193, 12),
                 (12289, 12),
                 (16385, 13),
                 (24577, 13)];
    table[code as usize]
}
fn decode_length(code: u16) -> (u16, u8) {
    let table = [(3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0), (11, 1),
                 (13, 1), (15, 1), (17, 1), (19, 2), (23, 2), (27, 2), (31, 2), (35, 3), (43, 3),
                 (51, 3), (59, 3), (67, 4), (83, 4), (99, 4), (115, 4), (131, 5), (163, 5),
                 (195, 5), (227, 5), (258, 0)];
    let index = (code - 257) as usize;
    table[index]
}

#[derive(Debug)]
pub enum Symbol {
    EndOfBlock,
    Literal(u8),

    // TODO: name
    Share { length: u16, distance: u16 },
}

#[cfg(test)]
mod test {
    #[test]
    fn it_works() {}
}
