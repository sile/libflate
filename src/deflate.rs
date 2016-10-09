use std::io;
use std::io::Read;
use std::cmp;
use std::iter;

use huffman;
use bit::BitReader;

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
        self.reader.align_u8();
        let len = try!(self.reader.read_u16());
        let nlen = try!(self.reader.read_u16());
        if !len != nlen {
            Err(io::Error::new(io::ErrorKind::InvalidData,
                               format!("LEN={} is not the one's complement of NLEN={}", len, nlen)))
        } else {
            self.block_buf.resize(len as usize, 0);
            self.block_offset = 0;
            try!(self.reader.as_inner_mut().read_exact(&mut self.block_buf));
            Ok(())
        }
    }
    fn decode3(&mut self, code: u16, v: &mut Vec<u8>) -> io::Result<()> {
        match code {
            0...15 => {
                v.push(code as u8);
            }
            16 => {
                let count = try!(self.reader.read_exact_bits(2)) + 3;
                let last = v.last().cloned().unwrap();
                v.extend(iter::repeat(last).take(count as usize));
            }
            17 => {
                let zeros = try!(self.reader.read_exact_bits(3)) + 3;
                v.extend(iter::repeat(0).take(zeros as usize));
            }
            18 => {
                let zeros = try!(self.reader.read_exact_bits(7)) + 11;
                v.extend(iter::repeat(0).take(zeros as usize));
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    fn read_compressed_block(&mut self, is_dynamic: bool) -> io::Result<()> {
        let mut huffman = if is_dynamic {
            let hlit = try!(self.reader.read_exact_bits(5)) + 257;
            let hdist = try!(self.reader.read_exact_bits(5)) + 1;
            let hclen = try!(self.reader.read_exact_bits(4)) + 4;

            let mut hc = [0; 19];
            let indices = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
            for &i in indices.iter().take(hclen as usize) {
                hc[i] = try!(self.reader.read_exact_bits(3)) as u8;
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
                    let start = self.block_buf.len() - distance as usize;
                    for i in start..start + length as usize {
                        let b = self.block_buf[i];
                        self.block_buf.push(b);
                    }

                    // use std::ptr;

                    // let src_start = self.block_buf.len() - distance as usize;
                    // let src_end = src_start + length as usize;
                    // let dst_start = self.block_buf.len();
                    // let dst_end = dst_start + length as usize;
                    // self.block_buf.resize(dst_end, 0);
                    // if src_end <= dst_start {
                    //     unsafe {
                    //         ptr::copy_nonoverlapping(self.block_buf[src_start..].as_ptr(),
                    //                                  self.block_buf[dst_start..].as_mut_ptr(),
                    //                                  length as usize);
                    //     };
                    // } else {
                    //     unsafe {
                    //         ptr::copy(self.block_buf[src_start..].as_ptr(),
                    //                   self.block_buf[dst_start..].as_mut_ptr(),
                    //                   length as usize);
                    //     };
                    // }
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
            let btype = try!(self.reader.read_exact_bits(2));
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
