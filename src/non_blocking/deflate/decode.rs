use std::io;
use std::io::Read;
use std::cmp;
// use std::mem;
use std::ptr;
// use byteorder::ReadBytesExt;
// use byteorder::LittleEndian;

use lz77;
use util;
use deflate::symbol::{self, HuffmanCodec};
use non_blocking::transaction::TransactionalBitReader;

#[derive(Debug)]
enum DecoderState {
    BlockHead,
    LoadFixedHuffmanCode,
    LoadDynamicHuffmanCode,
    DecodeBlock(symbol::Decoder),
}

#[derive(Debug)]
struct BlockDecoder {
    buffer: Vec<u8>,
    offset: usize,
    eob: bool,
}
impl BlockDecoder {
    pub fn new() -> Self {
        BlockDecoder {
            buffer: Vec::new(),
            offset: 0,
            eob: false,
        }
    }
    pub fn decode<R: Read>(
        &mut self,
        bit_reader: &mut TransactionalBitReader<R>,
        symbol_decoder: &mut symbol::Decoder,
    ) -> io::Result<()> {
        if self.eob {
            return Ok(());
        }

        while let Some(s) = self.decode_symbol(bit_reader, symbol_decoder)? {
            match s {
                symbol::Symbol::Literal(b) => {
                    self.buffer.push(b);
                }
                symbol::Symbol::Share { length, distance } => {
                    if self.buffer.len() < distance as usize {
                        return Err(invalid_data_error!(
                            "Too long backword reference: buffer.len={}, distance={}",
                            self.buffer.len(),
                            distance
                        ));
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
        Ok(())
    }
    pub fn enter_new_block(&mut self) {
        self.eob = false;
        self.truncate_old_buffer();
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
    fn decode_symbol<R: Read>(
        &mut self,
        bit_reader: &mut TransactionalBitReader<R>,
        symbol_decoder: &mut symbol::Decoder,
    ) -> io::Result<Option<symbol::Symbol>> {
        let result = bit_reader.transaction(|bit_reader| {
            let s = symbol_decoder.decode_unchecked(bit_reader);
            bit_reader.check_last_error().map(|()| s)
        });
        match result {
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
            Ok(s) => Ok(Some(s)),
        }
    }
}
impl Read for BlockDecoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.offset < self.buffer.len() {
            let copy_size = cmp::min(buf.len(), self.buffer.len() - self.offset);
            buf[..copy_size].copy_from_slice(&self.buffer[self.offset..][..copy_size]);
            self.offset += copy_size;
            Ok(copy_size)
        } else if self.eob {
            Ok(0)
        } else {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "Would block"))
        }
    }
}

#[derive(Debug)]
pub struct Decoder<R> {
    state: DecoderState,
    bit_reader: TransactionalBitReader<R>,
    eos: bool,
    block_decoder: BlockDecoder,
}
impl<R: Read> Decoder<R> {
    pub fn new(inner: R) -> Self {
        let reader = TransactionalBitReader::new(inner);
        Decoder {
            state: DecoderState::BlockHead,
            bit_reader: reader,
            eos: false,
            block_decoder: BlockDecoder::new(),
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
                    self.block_decoder.enter_new_block();
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
                    self.block_decoder.decode(
                        &mut self.bit_reader,
                        symbol_decoder,
                    )?;
                    let read_size = self.block_decoder.read(buf)?;
                    if read_size == 0 && !self.eos {
                        DecoderState::BlockHead
                    } else {
                        return Ok(read_size);
                    }
                }
            };
            self.state = next;
        }
    }
}

#[cfg(test)]
mod test {
    use std::io::{self, Read};
    use deflate::Encoder;
    use super::*;

    struct BlockReader<R> {
        inner: R,
        do_block: bool,
    }
    impl<R: Read> BlockReader<R> {
        pub fn new(inner: R) -> Self {
            BlockReader {
                inner,
                do_block: false,
            }
        }
    }
    impl<R: Read> Read for BlockReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.do_block = !self.do_block;
            if self.do_block {
                Err(io::Error::new(io::ErrorKind::WouldBlock, "Would block"))
            } else {
                let mut byte = [0; 1];
                if self.inner.read(&mut byte[..])? == 1 {
                    buf[0] = byte[0];
                    Ok(1)
                } else {
                    Ok(0)
                }
            }
        }
    }

    fn nb_read_to_end<R: Read>(mut reader: R) -> io::Result<Vec<u8>> {
        let mut buf = vec![0; 1024];
        let mut offset = 0;
        loop {
            match reader.read(&mut buf[offset..]) {
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(e);
                    }
                }
                Ok(0) => {
                    buf.truncate(offset);
                    break;
                }
                Ok(size) => {
                    offset += size;
                    if offset == buf.len() {
                        buf.resize(offset * 2, 0);
                    }
                }
            }
        }
        Ok(buf)
    }

    #[test]
    fn it_works() {
        let mut encoder = Encoder::new(Vec::new());
        io::copy(&mut &b"Hello World!"[..], &mut encoder).unwrap();
        let encoded_data = encoder.finish().into_result().unwrap();

        let mut decoder = Decoder::new(&encoded_data[..]);
        let mut decoded_data = Vec::new();
        decoder.read_to_end(&mut decoded_data).unwrap();

        assert_eq!(decoded_data, b"Hello World!");
    }

    #[test]
    fn nb_works() {
        let mut encoder = Encoder::new(Vec::new());
        io::copy(&mut &b"Hello World!"[..], &mut encoder).unwrap();
        let encoded_data = encoder.finish().into_result().unwrap();

        let decoder = Decoder::new(BlockReader::new(&encoded_data[..]));
        let decoded_data = nb_read_to_end(decoder).unwrap();

        assert_eq!(decoded_data, b"Hello World!");
    }
}
