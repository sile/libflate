/// Length-limited Huffman Codes
///
/// Reference: https://www.ics.uci.edu/~dan/pubs/LenLimHuff.pdf
use std::io;
use std::cmp;

use deflate::BitReader; // TODO: move

const CODE_UNDEF: u16 = 0;

pub struct Codes {
    min_len: u8,
    table: [u16; 0x10000],
}
impl Codes {
    fn new() -> Self {
        Codes {
            min_len: 0xFF,
            table: [CODE_UNDEF; 0x10000],
        }
    }
    fn set_mapping(&mut self, length: u8, from: u16, to: u16) {
        self.min_len = cmp::min(self.min_len, length);
        self.table[from as usize] = (to << 5) + (length as u16);
    }
    fn decode(&self, length: u8, code: u16) -> Option<u16> {
        let x = self.table[code as usize];
        if x & 0b11111 != length as u16 {
            return None;
        } else {
            Some(x >> 5)
        }
    }
}

pub fn fixed_literal_length_codes() -> Codes {
    let mut codes = Codes::new();
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
    let mut codes = Codes::new();
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

        let mut cs = Codes::new();
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
        let mut code = try!(reader.read_bit()) as u16;
        let mut length = 1;
        for _ in 0..16 {
            if let Some(decoded) = self.codes.decode(length, code) {
                return Ok(decoded);
            }
            code = (code << 1) | (try!(reader.read_bit()) as u16);
            length += 1;
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "TODO"))
    }
    pub fn codes(self) -> Codes {
        self.codes
    }
    pub fn min_len(&self) -> u8 {
        self.codes.min_len
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
        // let mut code = try!(reader.read_bit()) as u16;
        // let mut length = 1;

        let mut code = 0;
        let mut length = self.literal_codes.min_len;
        for _ in 0..length {
            code = (code << 1) | (try!(reader.read_bit()) as u16);
        }
        for _ in length..16 {
            if let Some(decoded) = self.literal_codes.decode(length, code) {
                // println!("! {}@{0:b}[{}] => {}", code, length, decoded);
                let s = match decoded {
                    0...255 => Symbol::Literal(decoded as u8),
                    256 => Symbol::EndOfBlock,
                    length_code => {
                        let (base, extra) = decode_length(length_code);
                        let length = base + try!(reader.read_bits_u8(extra)) as u16;
                        Symbol::Share {
                            length: length,
                            distance: 0,
                        }
                    }
                };
                return Ok(s);
            }
            code = (code << 1) | (try!(reader.read_bit()) as u16);
            length += 1;
        }
        Err(io::Error::new(io::ErrorKind::InvalidData,
                           "Can not decode literal or length code"))
    }
    fn decode_distance<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<u16>
        where R: io::Read
    {
        let mut code = try!(reader.read_bit()) as u16;
        let mut length = 1;
        for _ in 0..16 {
            if let Some(decoded) = self.distance_codes.decode(length, code) {
                // println!("@ {} => {}", code, decoded);
                let (base, extra) = decode_distance(decoded);
                // println!("# {}, {}", base, extra);
                let distance = base + try!(reader.read_bits_u16(extra)) as u16;
                return Ok(distance);
            }
            code = (code << 1) | (try!(reader.read_bit()) as u16);
            length += 1;
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "Can not decode distance code"))
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

fn decode_distance(code: u16) -> (u16, usize) {
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
fn decode_length(code: u16) -> (u16, usize) {
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
