// use std::cmp;
// use std::collections::HashMap;

pub const MAX_LENGTH: u16 = 258;
pub const MAX_DISTANCE: u16 = 32768;
pub const MAX_WINDOW_SIZE: u16 = 32768;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Code {
    Literal(u8),
    Pointer { length: u16, backward_distance: u16 },
}
impl Code {
    pub fn new_literal(symbol: u8) -> Self {
        Code::Literal(symbol)
    }
    pub fn new_pointer(length: u16, backward_distance: u16) -> Self {
        debug_assert!(length <= MAX_LENGTH);
        debug_assert!(backward_distance <= MAX_DISTANCE);
        Code::Pointer {
            length: length,
            backward_distance: backward_distance,
        }
    }
}

pub trait Sink {
    fn consume(&mut self, code: Code);
}
impl<'a, T> Sink for &'a mut T
    where T: Sink
{
    fn consume(&mut self, code: Code) {
        (*self).consume(code);
    }
}

// TODO: Lz77 or Lz77Encode or Lz77Compress
pub trait Encode {
    fn encode<S>(&mut self, buf: &[u8], sink: S) where S: Sink;
    fn flush<S>(&mut self, sink: S) where S: Sink;
    fn compression_mode(&self) -> CompressionMode {
        CompressionMode::default()
    }
    fn window_size(&self) -> u16 {
        MAX_WINDOW_SIZE
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CompressionMode {
    NoCompression,
    BestSpeed,
    Balance,
    BestCompression,
}
impl Default for CompressionMode {
    fn default() -> Self {
        CompressionMode::Balance
    }
}

// TODO: delete Clone, Default
#[derive(Debug, Clone, Default)]
pub struct DefaultEncoder;
impl Encode for DefaultEncoder {
    fn encode<S>(&mut self, buf: &[u8], mut sink: S)
        where S: Sink
    {
        for c in buf.iter().cloned().map(Code::Literal) {
            sink.consume(c);
        }
    }
    #[allow(unused_variables)]
    fn flush<S>(&mut self, sink: S) where S: Sink {}
}
