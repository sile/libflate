use std::cmp;
use std::collections::HashMap;

// TODO: improve compression
#[derive(Debug)]
pub struct Encoder {
    window_size: u16,
    buf: Vec<Symbol>,
    src: Vec<u8>,
}
impl Encoder {
    pub fn new(window_size: u16) -> Self {
        debug_assert!(window_size <= 0x8000);
        Encoder {
            window_size: window_size,
            buf: Vec::new(),
            src: Vec::new(),
        }
    }
    pub fn extend(&mut self, buf: &[u8]) {
        let room = self.window_size as usize - self.src.len();
        let extend_size = cmp::min(room, buf.len());
        self.src.extend_from_slice(&buf[..extend_size]);
        if self.src.len() == self.window_size as usize {
            self.lz77_encode();
        }
        if extend_size < buf.len() {
            self.extend(&buf[extend_size..]);
        }
    }
    pub fn drop(&mut self, size: usize) {
        debug_assert!(size <= self.buf.len());
        self.buf.drain(..size);
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn as_slice(&self) -> &[Symbol] {
        &self.buf[..]
    }
    // XXX: name
    pub fn flush(&mut self) {
        self.lz77_encode();
    }
    fn lz77_encode(&mut self) {
        let mut table: HashMap<[u8; 3], usize> = HashMap::new();
        let mut skips = 0;
        for i in 0..self.src.len() {
            if skips > 0 {
                skips -= 1;
                continue;
            }
            if i >= self.src.len() - 3 {
                self.buf.push(Symbol::Literal(self.src[i]));
                continue;
            }
            let prefix: [u8; 3] = [self.src[i], self.src[i + 1], self.src[i + 2]];
            if let Some(share_start) = table.get(&prefix) {
                let share_length = 3 +
                                   self.src[i + 3..]
                    .iter()
                    .take(255)
                    .enumerate()
                    .position(|(j, b)| *b != self.src[share_start + 3 + j])
                    .unwrap_or(0);
                skips = share_length - 1;
                self.buf.push(Symbol::Share {
                    length: share_length as u16,
                    distance: (i - share_start) as u16,
                });
            } else {
                self.buf.push(Symbol::Literal(prefix[0]));
            }
            table.insert(prefix, i);
        }
        self.src.clear();
    }
}

#[derive(Debug)]
pub enum Symbol {
    EndOfBlock,
    Literal(u8),
    Share { length: u16, distance: u16 },
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
