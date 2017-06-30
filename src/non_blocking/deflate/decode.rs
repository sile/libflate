use std::io;
use std::io::Read;
use std::cmp;
use std::mem;
use std::ptr;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;

use bit;
use lz77;
use util;
use deflate::symbol::{self, HuffmanCodec};
use non_blocking::bit::TransactionalBitReader;

#[derive(Debug)]
enum DecoderState {
    BlockHead,
    LoadFixedHuffmanCode,
    LoadDynamicHuffmanCode,
    DecodeBlock(symbol::Decoder),
}

#[derive(Debug)]
struct BlockHead<R> {
    bit_reader: Option<TransactionalBitReader<R>>,
}

#[derive(Debug)]
enum LoadHuffmanCode<R> {
    Fixed { bit_reader: TransactionalBitReader<R>, },
    Dynamic { bit_reader: TransactionalBitReader<R>, },
}

#[derive(Debug)]
struct DecodeBlock<R>(R);

#[derive(Debug)]
struct BlockDecoder<'a, 'b, R: 'a> {
    symbol_decoder: &'b mut symbol::Decoder,
    bit_reader: &'a mut TransactionalBitReader<R>,
    buffer: &'a mut Vec<u8>,
    offset: &'a mut usize,
    eob: bool,
}
impl<'a, 'b, R: 'a + Read> Read for BlockDecoder<'a, 'b, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if *self.offset < self.buffer.len() {
            let copy_size = cmp::min(buf.len(), self.buffer.len() - *self.offset);
            buf[..copy_size].copy_from_slice(&self.buffer[*self.offset..][..copy_size]);
            *self.offset += copy_size;
            return Ok(copy_size);
        }
        if self.eob {
            return Ok(0);
        }

        loop {
            let symbol_decoder = &mut self.symbol_decoder;
            let result = self.bit_reader.transaction(|bit_reader| {
                let s = symbol_decoder.decode_unchecked(bit_reader);
                bit_reader.check_last_error()?;
                Ok(s)
            });
            let s = match result {
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock && *self.offset < self.buffer.len() {
                        break;
                    }
                    return Err(e);
                }
                Ok(s) => s,
            };
            match s {
                symbol::Symbol::Literal(b) => {
                    self.buffer.push(b);
                }
                symbol::Symbol::Share { length, distance } => {
                    if self.buffer.len() < distance as usize {
                        let msg = format!(
                            "Too long backword reference: buffer.len={}, distance={}",
                            self.buffer.len(),
                            distance
                        );
                        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
                    }
                    let old_len = self.buffer.len();
                    self.buffer.reserve(length as usize);
                    unsafe {
                        self.buffer.set_len(old_len + length as usize);
                        let start = old_len - distance as usize;
                        let ptr = self.buffer.as_mut_ptr();
                        util::ptr_copy(
                            ptr.offset(start as isize),
                            ptr.offset(old_len as isize),
                            length as usize,
                            length > distance,
                        );
                    }
                }
                symbol::Symbol::EndOfBlock => {
                    self.eob = true;
                    break;
                }
            }
        }

        self.read(buf)
    }
}

#[derive(Debug)]
pub struct Decoder<R> {
    state: DecoderState,
    bit_reader: TransactionalBitReader<R>,
    eos: bool,
    buffer: Vec<u8>,
    offset: usize,
}
impl<R: Read> Decoder<R> {
    pub fn new(inner: R) -> Self {
        let reader = TransactionalBitReader::new(inner);
        Decoder {
            state: DecoderState::BlockHead,
            bit_reader: reader,
            eos: false,
            buffer: Vec::new(),
            offset: 0,
        }
    }
    fn truncate_old_buffer(&mut self) {
        if self.buffer.len() > lz77::MAX_DISTANCE as usize * 4 {
            let new_len = lz77::MAX_DISTANCE as usize;
            unsafe {
                let ptr = self.buffer.as_mut_ptr();
                let src = ptr.offset((self.buffer.len() - new_len) as isize);
                ptr::copy_nonoverlapping(src, ptr, new_len);
            }
            self.buffer.truncate(new_len);
            self.offset = new_len;
        }
    }
}
impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let next = match self.state {
                DecoderState::BlockHead => {
                    let (bfinal, btype) = self.bit_reader.transaction(|r| {
                        let bfinal = r.read_bit()?;
                        let btype = r.read_bits(2)?;
                        Ok((bfinal, btype))
                    })?;
                    self.eos = bfinal;
                    self.truncate_old_buffer();
                    match btype {
                        0b00 => unimplemented!(),
                        0b01 => DecoderState::LoadFixedHuffmanCode,
                        0b10 => DecoderState::LoadDynamicHuffmanCode,
                        0b11 => {
                            return Err(invalid_data_error!(
                                "btype 0x11 of DEFLATE is reserved(error) value"
                            ))
                        }
                        _ => unreachable!(),
                    }
                }
                DecoderState::LoadFixedHuffmanCode => {
                    let symbol_decoder = self.bit_reader.transaction(
                        |r| symbol::FixedHuffmanCodec.load(r),
                    )?;
                    DecoderState::DecodeBlock(symbol_decoder)
                }
                DecoderState::LoadDynamicHuffmanCode => {
                    let symbol_decoder = self.bit_reader.transaction(
                        |r| symbol::DynamicHuffmanCodec.load(r),
                    )?;
                    DecoderState::DecodeBlock(symbol_decoder)
                }
                DecoderState::DecodeBlock(ref mut symbol_decoder) => {
                    let mut decoder = BlockDecoder {
                        symbol_decoder,
                        bit_reader: &mut self.bit_reader,
                        buffer: &mut self.buffer,
                        offset: &mut self.offset,
                        eob: false,
                    };
                    let read_size = decoder.read(buf)?;
                    if read_size != 0 || self.eos {
                        return Ok(read_size);
                    }
                    DecoderState::BlockHead
                }
            };
            self.state = next;
        }
    }
}
