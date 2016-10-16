use std::io;
use std::io::Read;
use std::cmp;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;

use bit;
use lz77;
use super::symbol;

/// DEFLATE decoder.
#[derive(Debug)]
pub struct Decoder<R> {
    bit_reader: bit::BitReader<R>,
    buffer: Vec<u8>,
    offset: usize,
    eos: bool,
}
impl<R> Decoder<R>
    where R: Read
{
    /// Makes a new decoder instance.
    ///
    /// `inner` is to be decoded DEFLATE stream.
    ///
    /// # Examples
    /// ```
    /// use std::io::{Cursor, Read};
    /// use libflate::deflate::Decoder;
    ///
    /// let encoded_data = [243, 72, 205, 201, 201, 87, 8, 207, 47, 202, 73, 81, 4, 0];
    /// let mut decoder = Decoder::new(Cursor::new(&encoded_data));
    /// let mut buf = Vec::new();
    /// decoder.read_to_end(&mut buf).unwrap();
    ///
    /// assert_eq!(buf, b"Hello World!");
    /// ```
    pub fn new(inner: R) -> Self {
        Decoder {
            bit_reader: bit::BitReader::new(inner),
            buffer: Vec::new(),
            offset: 0,
            eos: false,
        }
    }

    /// Returns the immutable reference to the inner stream.
    pub fn as_inner_ref(&self) -> &R {
        self.bit_reader.as_inner_ref()
    }

    /// Returns the mutable reference to the inner stream.
    pub fn as_inner_mut(&mut self) -> &mut R {
        self.bit_reader.as_inner_mut()
    }

    /// Unwraps this `Decoder`, returning the underlying reader.
    ///
    /// # Examples
    /// ```
    /// use std::io::Cursor;
    /// use libflate::deflate::Decoder;
    ///
    /// let encoded_data = [243, 72, 205, 201, 201, 87, 8, 207, 47, 202, 73, 81, 4, 0];
    /// let decoder = Decoder::new(Cursor::new(&encoded_data));
    /// assert_eq!(decoder.into_inner().into_inner(), &encoded_data);
    /// ```
    pub fn into_inner(self) -> R {
        self.bit_reader.into_inner()
    }

    fn read_non_compressed_block(&mut self) -> io::Result<()> {
        self.bit_reader.reset();
        let len = try!(self.bit_reader.as_inner_mut().read_u16::<LittleEndian>());
        let nlen = try!(self.bit_reader.as_inner_mut().read_u16::<LittleEndian>());
        if !len != nlen {
            Err(invalid_data_error!("LEN={} is not the one's complement of NLEN={}", len, nlen))
        } else {
            let old_len = self.buffer.len();
            self.buffer.resize(old_len + len as usize, 0);
            try!(self.bit_reader.as_inner_mut().read_exact(&mut self.buffer[old_len..]));
            Ok(())
        }
    }
    fn read_compressed_block<H>(&mut self, huffman: H) -> io::Result<()>
        where H: symbol::HuffmanCodec
    {
        let symbol_decoder = try!(huffman.load(&mut self.bit_reader));
        loop {
            let s = try!(symbol_decoder.decode(&mut self.bit_reader));
            match s {
                symbol::Symbol::Literal(b) => {
                    self.buffer.push(b);
                }
                symbol::Symbol::Share { length, distance } => {
                    let start = self.buffer.len() - distance as usize;
                    for i in (start..).take(length as usize) {
                        let b = unsafe { *self.buffer.get_unchecked(i) };
                        self.buffer.push(b);
                    }
                }
                symbol::Symbol::EndOfBlock => {
                    break;
                }
            }
        }
        Ok(())
    }
    fn truncate_old_buffer(&mut self) {
        if self.buffer.len() > lz77::MAX_DISTANCE as usize * 4 {
            let new_start = self.buffer.len() - lz77::MAX_DISTANCE as usize;
            self.buffer.drain(0..new_start);
            self.offset = lz77::MAX_DISTANCE as usize;
        }
    }
}
impl<R> Read for Decoder<R>
    where R: Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.offset < self.buffer.len() {
            let copy_size = cmp::min(buf.len(), self.buffer.len() - self.offset);
            buf[..copy_size].copy_from_slice(&self.buffer[self.offset..][..copy_size]);
            self.offset += copy_size;
            Ok(copy_size)
        } else if self.eos {
            Ok(0)
        } else {
            let bfinal = try!(self.bit_reader.read_bit());
            let btype = try!(self.bit_reader.read_bits(2));
            self.eos = bfinal;
            self.truncate_old_buffer();
            match btype {
                0b00 => {
                    try!(self.read_non_compressed_block());
                    self.read(buf)
                }
                0b01 => {
                    try!(self.read_compressed_block(symbol::FixedHuffmanCodec));
                    self.read(buf)
                }
                0b10 => {
                    try!(self.read_compressed_block(symbol::DynamicHuffmanCodec));
                    self.read(buf)
                }
                0b11 => Err(invalid_data_error!("btype 0x11 of DEFLATE is reserved(error) value")),
                _ => unreachable!(),
            }
        }
    }
}
