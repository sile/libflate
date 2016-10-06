use std::io;
use std::io::Read;
use std::cmp;
use std::iter;
use byteorder::ReadBytesExt;

use huffman;

pub struct Decoder<R> {
    reader: BitReader<R>,
    block_buf: Vec<u8>,
    block_offset: usize,
    eos: bool,
}
impl<R> Decoder<R>
    where R: Read
{
    pub fn new(reader: R) -> Self {
        Decoder {
            reader: BitReader::new(reader),
            block_buf: Vec::new(),
            block_offset: 0,
            eos: false,
        }
    }
    pub fn as_inner_mut(&mut self) -> &mut R {
        self.reader.as_inner_mut()
    }
    pub fn into_reader(self) -> R {
        self.reader.into_byte_reader()
    }
    fn read_non_compressed_block(&mut self) -> io::Result<()> {
        let len = try!(self.reader.read_byte_aligned_u16());
        let nlen = try!(self.reader.read_byte_aligned_u16());
        if !len != nlen {
            Err(io::Error::new(io::ErrorKind::InvalidData,
                               format!("LEN={} is not the one's complement of NLEN={}", len, nlen)))
        } else {
            self.block_buf.resize(len as usize, 0);
            self.block_offset = 0;
            try!(self.reader.byte_reader.read_exact(&mut self.block_buf));
            Ok(())
        }
    }
    fn decode3(&mut self, code: u16, v: &mut Vec<u8>) -> io::Result<()> {
        match code {
            0...15 => {
                v.push(code as u8);
            }
            16 => {
                let count = try!(self.reader.read_bits_u8(2)) + 3;
                let last = v.last().cloned().unwrap();
                v.extend(iter::repeat(last).take(count as usize));
            }
            17 => {
                let zeros = try!(self.reader.read_bits_u8(3)) + 3;
                v.extend(iter::repeat(0).take(zeros as usize));
            }
            18 => {
                let zeros = try!(self.reader.read_bits_u8(7)) + 11;
                v.extend(iter::repeat(0).take(zeros as usize));
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    fn read_compressed_block(&mut self, is_dynamic: bool) -> io::Result<()> {
        let mut huffman = if is_dynamic {
            let hlit = try!(self.reader.read_bits_u8(5)) as u16 + 257;
            let hdist = try!(self.reader.read_bits_u8(5)) + 1;
            let hclen = try!(self.reader.read_bits_u8(4)) + 4;

            let mut hc = [0; 19];
            let indices = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
            for &i in indices.iter().take(hclen as usize) {
                hc[i] = try!(self.reader.read_bits_u8(3));
            }
            let mut code_length_codes = huffman::Decoder2::from_lens(&hc[..]);

            let mut lit_lens = Vec::with_capacity(hlit as usize);
            while lit_lens.len() < hlit as usize {
                let c = try!(code_length_codes.decode(&mut self.reader));
                try!(self.decode3(c, &mut lit_lens));
            }
            let lite_codes = huffman::Decoder2::from_lens(&lit_lens[..]);

            let mut dist_lens = Vec::with_capacity(hdist as usize);
            while dist_lens.len() < hdist as usize {
                let c = try!(code_length_codes.decode(&mut self.reader));
                try!(self.decode3(c, &mut dist_lens));
            }
            let dist_codes = huffman::Decoder2::from_lens(&dist_lens[..]);

            huffman::Decoder::new(lite_codes.codes(), dist_codes.codes())
        } else {
            huffman::Decoder::new_fixed()
        };
        loop {
            let s = try!(huffman.decode_one(&mut self.reader));
            match s {
                huffman::Symbol::Literal(b) => {
                    self.block_buf.push(b);
                }
                huffman::Symbol::Share { length, distance } => {
                    // TODO: optimize
                    let start = self.block_buf.len() - distance as usize;
                    let tmp = self.block_buf[start..][..length as usize]
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    self.block_buf.extend(tmp);
                }
                huffman::Symbol::EndOfBlock => {
                    break;
                }
            }
        }
        Ok(())
    }
}
impl<R> Read for Decoder<R>
    where R: Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.block_offset < self.block_buf.len() {
            let copy_size = cmp::min(buf.len(), self.block_buf.len() - self.block_offset);
            buf[..copy_size].copy_from_slice(&self.block_buf[self.block_offset..][..copy_size]);
            self.block_offset += copy_size;
            Ok(copy_size)
        } else if self.eos {
            Ok(0)
        } else {
            let bfinal = try!(self.reader.read_bit());
            let btype = try!(self.reader.read_bits_u8(2));
            self.eos = bfinal;
            match btype {
                0b00 => {
                    try!(self.read_non_compressed_block());
                    self.read(buf)
                }
                0b01 => {
                    try!(self.read_compressed_block(false));
                    self.read(buf)
                }
                0b10 => {
                    try!(self.read_compressed_block(true));
                    self.read(buf)
                }
                0b11 => {
                    Err(io::Error::new(io::ErrorKind::InvalidData,
                                       "btype 0x11 of DEFLATE is reserved(error) value"))
                }
                _ => unreachable!(),
            }
        }
    }
}

pub struct BitReader<R> {
    byte_reader: R,
    last_byte: u8,
    offset: usize,
}
impl<R> BitReader<R>
    where R: Read
{
    pub fn new(byte_reader: R) -> Self {
        BitReader {
            byte_reader: byte_reader,
            last_byte: 0,
            offset: 8,
        }
    }
    pub fn as_inner_mut(&mut self) -> &mut R {
        &mut self.byte_reader
    }
    pub fn into_byte_reader(self) -> R {
        self.byte_reader
    }
    pub fn read_bit(&mut self) -> io::Result<bool> {
        if self.offset == 8 {
            self.last_byte = try!(self.byte_reader.read_u8());
            self.offset = 0;
        }
        let bit = (self.last_byte & (1 << self.offset)) != 0;
        self.offset += 1;
        Ok(bit)
    }
    pub fn read_bits_u8(&mut self, bits: usize) -> io::Result<u8> {
        assert!(bits <= 8);
        // TODO: optimize
        let mut n = 0;
        for i in 0..bits {
            let bit = try!(self.read_bit());
            n |= (bit as u8) << i;
        }
        Ok(n)
    }
    pub fn read_bits_u16(&mut self, bits: usize) -> io::Result<u16> {
        assert!(bits <= 16);
        // TODO: optimize
        let mut n = 0;
        for i in 0..bits {
            let bit = try!(self.read_bit());
            n |= (bit as u16) << i;
        }
        Ok(n)
    }
    pub fn read_byte_aligned_u16(&mut self) -> io::Result<u16> {
        if self.offset != 0 {
            self.last_byte = try!(self.byte_reader.read_u8());
        }
        self.offset = 8;
        let low = self.last_byte as u16;
        let high = try!(self.byte_reader.read_u8()) as u16;
        Ok((high << 8) | low)
    }
}
