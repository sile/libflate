use std::io;
use std::cmp;
use std::iter;
use std::ops::Range;

use bit;
use huffman;
use huffman::Builder;
use super::Symbol;

const FIXED_LITERAL_OR_LENGTH_CODE_TABLE: [(u8, Range<u16>, u16); 4] =
    [(8, 000..144, 0b0_0011_0000),
     (9, 144..256, 0b1_1001_0000),
     (7, 256..280, 0b0_0000_0000),
     (8, 280..288, 0b0_1100_0000)];

const BITWIDTH_CODE_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2,
                                          14, 1, 15];
const EOB_SYMBOL: u16 = 256; // TODO: move

#[derive(Debug)]
pub struct SymbolCodes<T> {
    pub literal: T,
    pub distance: T,
}

pub trait Factory {
    fn build_codes(&self, symbols: &[Symbol]) -> SymbolCodes<huffman::Encoder>;
    fn save<W>(&self,
               writer: &mut bit::BitWriter<W>,
               codes: &SymbolCodes<huffman::Encoder>)
               -> io::Result<()>
        where W: io::Write;
    fn load<R>(&self, reader: &mut bit::BitReader<R>) -> io::Result<SymbolCodes<huffman::Decoder>>
        where R: io::Read;
}

#[derive(Debug)]
pub struct Fixed;
impl Factory for Fixed {
    #[allow(unused_variables)]
    fn build_codes(&self, symbols: &[Symbol]) -> SymbolCodes<huffman::Encoder> {
        let (literal, distance) = fixed_encoders();
        SymbolCodes {
            literal: literal,
            distance: distance,
        }
    }
    #[allow(unused_variables)]
    fn save<W>(&self,
               writer: &mut bit::BitWriter<W>,
               codes: &SymbolCodes<huffman::Encoder>)
               -> io::Result<()>
        where W: io::Write
    {
        Ok(())
    }
    #[allow(unused_variables)]
    fn load<R>(&self, reader: &mut bit::BitReader<R>) -> io::Result<SymbolCodes<huffman::Decoder>>
        where R: io::Read
    {
        let (literal, distance) = fixed_decoders();
        Ok(SymbolCodes {
            literal: literal,
            distance: distance,
        })
    }
}

#[derive(Debug)]
pub struct Dynamic;
impl Factory for Dynamic {
    fn build_codes(&self, symbols: &[Symbol]) -> SymbolCodes<huffman::Encoder> {
        let mut literal_counts = [0; 286];
        let mut distance_counts = [0; 30];
        for s in symbols {
            literal_counts[s.code() as usize] += 1;
            if let Some((d, _, _)) = s.distance() {
                distance_counts[d as usize] += 1;
            }
        }
        SymbolCodes {
            literal: huffman::EncoderBuilder::from_frequencies(&literal_counts, 15),
            distance: huffman::EncoderBuilder::from_frequencies(&distance_counts, 15),
        }
    }
    fn save<W>(&self,
               writer: &mut bit::BitWriter<W>,
               codes: &SymbolCodes<huffman::Encoder>)
               -> io::Result<()>
        where W: io::Write
    {
        save_dynamic_codes(writer, &codes.literal, &codes.distance)
    }
    fn load<R>(&self, reader: &mut bit::BitReader<R>) -> io::Result<SymbolCodes<huffman::Decoder>>
        where R: io::Read
    {
        let (literal, distance) = try!(load_dynamic_decoders(reader));
        Ok(SymbolCodes {
            literal: literal,
            distance: distance,
        })
    }
}

pub fn fixed_encoders() -> (huffman::Encoder, huffman::Encoder) {
    let mut literal_builder = huffman::EncoderBuilder::new(288);
    for &(bitwidth, ref symbols, code_base) in &FIXED_LITERAL_OR_LENGTH_CODE_TABLE {
        for (code, symbol) in symbols.clone().enumerate().map(|(i, s)| (code_base + i as u16, s)) {
            literal_builder.set_mapping(symbol, huffman::Code::new(bitwidth, code));
        }
    }

    let mut distance_builder = huffman::EncoderBuilder::new(30);
    for i in 0..30 {
        distance_builder.set_mapping(i, huffman::Code::new(5, i));
    }

    (literal_builder.finish(), distance_builder.finish())
}

pub fn fixed_decoders() -> (huffman::Decoder, huffman::Decoder) {
    let mut literal_builder = huffman::DecoderBuilder::new(9, Some(EOB_SYMBOL));
    for &(bitwidth, ref symbols, code_base) in &FIXED_LITERAL_OR_LENGTH_CODE_TABLE {
        for (code, symbol) in symbols.clone().enumerate().map(|(i, s)| (code_base + i as u16, s)) {
            literal_builder.set_mapping(symbol, huffman::Code::new(bitwidth, code));
        }
    }

    let mut distance_builder = huffman::DecoderBuilder::new(5, None);
    for i in 0..30 {
        distance_builder.set_mapping(i, huffman::Code::new(5, i));
    }

    (literal_builder.finish(), distance_builder.finish())
}

pub fn load_dynamic_decoders<R>(reader: &mut bit::BitReader<R>)
                                -> io::Result<(huffman::Decoder, huffman::Decoder)>
    where R: io::Read
{
    let literal_code_count = try!(reader.read_bits(5)) + 257;
    let distance_code_count = try!(reader.read_bits(5)) + 1;
    let bitwidth_code_count = try!(reader.read_bits(4)) + 4;

    let mut bitwidth_code_bitwidthes = [0; 19];
    for &i in BITWIDTH_CODE_ORDER.iter().take(bitwidth_code_count as usize) {
        bitwidth_code_bitwidthes[i] = try!(reader.read_bits(3)) as u8;
    }
    let mut bitwidth_decoder = huffman::DecoderBuilder::from_bitwidthes(&bitwidth_code_bitwidthes,
                                                                        None);

    let mut literal_code_bitwidthes = Vec::with_capacity(literal_code_count as usize);
    while literal_code_bitwidthes.len() < literal_code_count as usize {
        let c = try!(bitwidth_decoder.decode(reader));
        let last = literal_code_bitwidthes.last().cloned();
        literal_code_bitwidthes.extend(try!(load_bitwidthes(reader, c, last)));
    }

    let mut distance_code_bitwidthes = Vec::with_capacity(distance_code_count as usize);
    while distance_code_bitwidthes.len() < distance_code_count as usize {
        let c = try!(bitwidth_decoder.decode(reader));
        let last = distance_code_bitwidthes.last().cloned();
        distance_code_bitwidthes.extend(try!(load_bitwidthes(reader, c, last)));
    }

    Ok((huffman::DecoderBuilder::from_bitwidthes(&literal_code_bitwidthes, Some(EOB_SYMBOL)),
        huffman::DecoderBuilder::from_bitwidthes(&distance_code_bitwidthes, None)))
}

fn load_bitwidthes<R>(reader: &mut bit::BitReader<R>,
                      code: u16,
                      last: Option<u8>)
                      -> io::Result<Box<Iterator<Item = u8>>>
    where R: io::Read
{
    Ok(match code {
        0...15 => Box::new(iter::once(code as u8)),
        16 => {
            let count = try!(reader.read_bits(2)) + 3;
            let last = try!(last.ok_or_else(|| invalid_data_error!("No preceeding value")));
            Box::new(iter::repeat(last).take(count as usize))
        }
        17 => {
            let zeros = try!(reader.read_bits(3)) + 3;
            Box::new(iter::repeat(0).take(zeros as usize))
        }
        18 => {
            let zeros = try!(reader.read_bits(7)) + 11;
            Box::new(iter::repeat(0).take(zeros as usize))
        }
        _ => unreachable!(),
    })
}

// TODO: refactor
pub fn save_dynamic_codes<W>(writer: &mut bit::BitWriter<W>,
                             literal_encoder: &huffman::Encoder,
                             distance_encoder: &huffman::Encoder)
                             -> io::Result<()>
    where W: io::Write
{
    struct Sym {
        value: u8,
        count: usize,
    }

    let literal_code_count = cmp::max(257, literal_encoder.used_max_symbol().unwrap_or(0) + 1);
    let distance_code_count = cmp::max(1, distance_encoder.used_max_symbol().unwrap_or(0) + 1);

    let mut syms: Vec<Sym> = Vec::new();
    for &(e, size) in &[(&literal_encoder, literal_code_count),
                        (&distance_encoder, distance_code_count)] {
        for (i, c) in (0..size).map(|x| e.lookup(x as u16).width).enumerate() {
            if i > 0 && syms.last().map_or(false, |s| s.value == c) {
                syms.last_mut().unwrap().count += 1;
            } else {
                syms.push(Sym {
                    value: c,
                    count: 1,
                })
            }
        }
    }

    let mut codes = Vec::new();
    for s in &syms {
        if s.value == 0 {
            let mut c = s.count;
            while c >= 11 {
                let n = cmp::min(138, c);
                codes.push((18, 7, n - 11));
                c -= n;
            }
            if c >= 3 {
                codes.push((17, 3, c - 3));
                c = 0;
            }
            for _ in 0..c {
                codes.push((0, 0, 0));
            }
        } else {
            codes.push((s.value, 0, 0));
            let mut c = s.count - 1;
            while c >= 3 {
                let n = cmp::min(6, c);
                codes.push((16, 2, n - 3));
                c -= n;
            }
            for _ in 0..c {
                codes.push((s.value, 0, 0));
            }
        }
    }

    let mut code_counts = [0; 19];
    for x in &codes {
        code_counts[x.0 as usize] += 1;
    }
    let mut bitwidth_encoder = huffman::EncoderBuilder::from_frequencies(&code_counts, 7);
    let bitwidth_code_count =
        cmp::max(4,
                 BITWIDTH_CODE_ORDER.iter()
                     .rev()
                     .position(|&i| bitwidth_encoder.lookup(i as u16).width > 0)
                     .map_or(0, |trailing_zeros| 19 - trailing_zeros)) as u16;
    try!(writer.write_bits(5, literal_code_count - 257));
    try!(writer.write_bits(5, distance_code_count - 1));
    try!(writer.write_bits(4, bitwidth_code_count - 4));
    for &i in BITWIDTH_CODE_ORDER.iter().take(bitwidth_code_count as usize) {
        try!(writer.write_bits(3, bitwidth_encoder.lookup(i as u16).width as u16));
    }
    for &(code, bits, extra) in &codes {
        try!(bitwidth_encoder.encode(writer, code as u16));
        if bits > 0 {
            try!(writer.write_bits(bits, extra as u16));
        }
    }
    Ok(())
}
