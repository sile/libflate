use std::io;
use std::io::Read;
use std::cmp;
use std::ptr;
use std::iter;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;

use huffman;
use bit::BitReader;
use super::Symbol;

const MAX_DISTANCE: usize = 0x8000;

#[derive(Debug)]
pub struct Decoder<R> {
    bit_reader: BitReader<R>,
    buffer: Vec<u8>,
    offset: usize,
    eos: bool,
}
impl<R> Decoder<R>
    where R: Read
{
    pub fn new(inner: R) -> Self {
        Decoder {
            bit_reader: BitReader::new(inner),
            buffer: Vec::new(),
            offset: 0,
            eos: false,
        }
    }
    pub fn as_inner_ref(&self) -> &R {
        self.bit_reader.as_inner_ref()
    }
    pub fn as_inner_mut(&mut self) -> &mut R {
        self.bit_reader.as_inner_mut()
    }
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
    fn read_compressed_block(&mut self, is_dynamic: bool) -> io::Result<()> {
        let mut huffman = if is_dynamic {
            try!(self.read_dynamic_huffman_codes())
        } else {
            SymbolDecoder::new_fixed()
        };
        loop {
            let s = try!(huffman.decode(&mut self.bit_reader));
            match s {
                Symbol::Literal(b) => {
                    self.buffer.push(b);
                }
                Symbol::Share { length, distance } => {
                    let start = self.buffer.len() - distance as usize;
                    for i in (start..).take(length as usize) {
                        let b = unsafe { *self.buffer.get_unchecked(i) };
                        self.buffer.push(b);
                    }
                }
                Symbol::EndOfBlock => {
                    break;
                }
            }
        }
        Ok(())
    }
    fn read_dynamic_huffman_codes(&mut self) -> io::Result<SymbolDecoder> {
        let literal_code_count = try!(self.bit_reader.read_bits(5)) + 257;
        let distance_code_count = try!(self.bit_reader.read_bits(5)) + 1;
        let bitwidth_code_count = try!(self.bit_reader.read_bits(4)) + 4;

        let mut bitwidth_code_bitwidthes = [0; 19];
        let indices = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
        for &i in indices.iter().take(bitwidth_code_count as usize) {
            bitwidth_code_bitwidthes[i] = try!(self.bit_reader.read_bits(3)) as u8;
        }
        let mut bitwidth_decoder =
            huffman::DecoderBuilder::from_bitwidthes(&bitwidth_code_bitwidthes);

        let mut literal_code_bitwidthes = Vec::with_capacity(literal_code_count as usize);
        while literal_code_bitwidthes.len() < literal_code_count as usize {
            let c = try!(bitwidth_decoder.decode(&mut self.bit_reader));
            try!(self.decode_bitwidthes(c, &mut literal_code_bitwidthes));
        }

        let mut distance_code_bitwidthes = Vec::with_capacity(distance_code_count as usize);
        while distance_code_bitwidthes.len() < distance_code_count as usize {
            let c = try!(bitwidth_decoder.decode(&mut self.bit_reader));
            try!(self.decode_bitwidthes(c, &mut distance_code_bitwidthes));
        }

        let literal_decoder = huffman::DecoderBuilder::from_bitwidthes(&literal_code_bitwidthes);
        let distance_decoder = huffman::DecoderBuilder::from_bitwidthes(&distance_code_bitwidthes);
        Ok(SymbolDecoder::new(literal_decoder, distance_decoder))
    }
    fn decode_bitwidthes(&mut self, code: u16, bitwidthes: &mut Vec<u8>) -> io::Result<()> {
        match code {
            0...15 => {
                bitwidthes.push(code as u8);
            }
            16 => {
                let count = try!(self.bit_reader.read_bits(2)) + 3;
                let last = try!(bitwidthes.last()
                    .cloned()
                    .ok_or_else(|| invalid_data_error!("No preceeding value")));
                bitwidthes.extend(iter::repeat(last).take(count as usize));
            }
            17 => {
                let zeros = try!(self.bit_reader.read_bits(3)) + 3;
                bitwidthes.extend(iter::repeat(0).take(zeros as usize));
            }
            18 => {
                let zeros = try!(self.bit_reader.read_bits(7)) + 11;
                bitwidthes.extend(iter::repeat(0).take(zeros as usize));
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    fn truncate_old_buffer(&mut self) {
        if self.buffer.len() > MAX_DISTANCE * 4 {
            let new_start = self.buffer.len() - MAX_DISTANCE;
            unsafe {
                ptr::copy_nonoverlapping(self.buffer[new_start..].as_ptr(),
                                         self.buffer[..].as_mut_ptr(),
                                         MAX_DISTANCE);
            }
            self.buffer.truncate(MAX_DISTANCE);
            self.offset = MAX_DISTANCE;
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
                    try!(self.read_compressed_block(false));
                    self.read(buf)
                }
                0b10 => {
                    try!(self.read_compressed_block(true));
                    self.read(buf)
                }
                0b11 => Err(invalid_data_error!("btype 0x11 of DEFLATE is reserved(error) value")),
                _ => unreachable!(),
            }
        }
    }
}

pub struct SymbolDecoder {
    literal_decoder: huffman::Decoder,
    distance_decoder: huffman::Decoder,
}
impl SymbolDecoder {
    pub fn new(lite: huffman::Decoder, dist: huffman::Decoder) -> Self {
        SymbolDecoder {
            literal_decoder: lite,
            distance_decoder: dist,
        }
    }
    pub fn new_fixed() -> Self {
        let mut literal_builder = huffman::DecoderBuilder::new(9);
        for i in 0..144 {
            literal_builder.set_mapping(8, 0b0011_0000 + i, i);
        }
        for i in 144..256 {
            literal_builder.set_mapping(9, 0b1_1001_0000 + i - 144, i);
        }
        for i in 256..280 {
            literal_builder.set_mapping(7, 0b000_0000 + i - 256, i);
        }
        for i in 280..287 {
            literal_builder.set_mapping(8, 0b1100_0000 + i - 280, i);
        }

        let mut distance_builder = huffman::DecoderBuilder::new(5);
        for i in 0..30 {
            distance_builder.set_mapping(5, i, i);
        }

        SymbolDecoder {
            literal_decoder: literal_builder.finish(),
            distance_decoder: distance_builder.finish(),
        }
    }
    fn decode<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<Symbol>
        where R: io::Read
    {
        self.decode_literal_or_length(reader).and_then(|mut s| {
            if let Symbol::Share { ref mut distance, .. } = s {
                *distance = try!(self.decode_distance(reader));
            }
            Ok(s)
        })
    }
    fn decode_literal_or_length<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<Symbol>
        where R: io::Read
    {
        let decoded = try!(self.literal_decoder.decode(reader));
        match decoded {
            0...255 => Ok(Symbol::Literal(decoded as u8)),
            256 => Ok(Symbol::EndOfBlock),
            length_code => {
                let (base, extra_bits) =
                    unsafe { *LENGTH_TABLE.get_unchecked(length_code as usize - 257) };
                let extra = try!(reader.read_bits(extra_bits));
                Ok(Symbol::Share {
                    length: base + extra,
                    distance: 0,
                })
            }
        }
    }
    fn decode_distance<R>(&mut self, reader: &mut BitReader<R>) -> io::Result<u16>
        where R: io::Read
    {
        let decoded = try!(self.distance_decoder.decode(reader)) as usize;
        let (base, extra_bits) = unsafe { *DISTANCE_TABLE.get_unchecked(decoded) };
        let extra = try!(reader.read_bits(extra_bits));
        let distance = base + extra;
        Ok(distance)
    }
}

const LENGTH_TABLE: [(u16, u8); 29] =
    [(3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0), (11, 1), (13, 1), (15, 1),
     (17, 1), (19, 2), (23, 2), (27, 2), (31, 2), (35, 3), (43, 3), (51, 3), (59, 3), (67, 4),
     (83, 4), (99, 4), (115, 4), (131, 5), (163, 5), (195, 5), (227, 5), (258, 0)];

const DISTANCE_TABLE: [(u16, u8); 30] = [(1, 0),
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
