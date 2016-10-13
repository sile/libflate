pub use self::decode::Decoder;
pub use self::encode::Encoder;
pub use self::encode::EncodeOptions;

use lz77;

mod decode;
mod encode;
mod codes;

#[derive(Debug, Clone, Copy)]
enum BlockType {
    Raw = 0b00,
    Fixed = 0b01,
    Dynamic = 0b10,
}

// TODO: private and ...
#[derive(Debug)]
pub enum Symbol {
    EndOfBlock,
    Literal(u8),
    Share { length: u16, distance: u16 },
}
impl From<lz77::Code> for Symbol {
    fn from(c: lz77::Code) -> Self {
        match c {
            lz77::Code::Literal(b) => Symbol::Literal(b),
            lz77::Code::Pointer { length, backward_distance } => {
                Symbol::Share {
                    length: length,
                    distance: backward_distance,
                }
            }
        }
    }
}
impl Symbol {
    pub fn code(&self) -> u16 {
        match *self {
            Symbol::Literal(b) => b as u16,
            Symbol::EndOfBlock => 256,
            Symbol::Share { length, .. } => {
                match length {
                    3...10 => 257 + length - 3,
                    11...18 => 265 + (length - 11) / 2,
                    19...34 => 269 + (length - 19) / 4,
                    35...66 => 273 + (length - 35) / 8,
                    67...130 => 277 + (length - 67) / 16,
                    131...257 => 281 + (length - 131) / 32,
                    258 => 285,
                    _ => unreachable!(),
                }
            }
        }
    }
    pub fn extra_lengh(&self) -> Option<(u8, u16)> {
        if let Symbol::Share { length, .. } = *self {
            match length {
                3...10 => None,
                11...18 => Some((1, (length - 11) % 2)),
                19...34 => Some((2, (length - 19) % 4)),
                35...66 => Some((3, (length - 35) % 8)),
                67...130 => Some((4, (length - 67) % 16)),
                131...257 => Some((5, (length - 131) % 32)),
                258 => None,
                _ => unreachable!(),
            }
        } else {
            None
        }
    }
    pub fn distance(&self) -> Option<(u8, u8, u16)> {
        if let Symbol::Share { distance, .. } = *self {
            if distance <= 4 {
                Some((distance as u8 - 1, 0, 0))
            } else {
                let mut extra_bits = 1;
                let mut code = 4;
                let mut base = 4;
                while base * 2 < distance {
                    extra_bits += 1;
                    code += 2;
                    base *= 2;
                }
                let half = base / 2;
                let delta = distance - base - 1;
                if distance <= base + half {
                    Some((code, extra_bits, delta % half))
                } else {
                    Some((code + 1, extra_bits, delta % half))
                }
            }
        } else {
            None
        }
    }
}
