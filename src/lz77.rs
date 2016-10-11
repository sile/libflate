use std::cmp;
use std::collections::HashMap;

use deflate::Symbol;

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
