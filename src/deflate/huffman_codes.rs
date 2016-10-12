use std::io;
use std::cmp;
use std::iter;
use std::ops::Range;

use bit;
use huffman;

const FIXED_LITERAL_OR_LENGTH_CODE_TABLE: [(u8, Range<u16>, u16); 4] =
    [(8, 000..144, 0b0_0011_0000),
     (9, 144..256, 0b1_1001_0000),
     (7, 256..280, 0b0_0000_0000),
     (8, 280..288, 0b0_1100_0000)];

pub fn fixed_encoders() -> (huffman::Encoder, huffman::Encoder) {
    let mut literal_builder = huffman::EncoderBuilder::new(288);
    for &(bitwidth, ref symbols, code_base) in &FIXED_LITERAL_OR_LENGTH_CODE_TABLE {
        for (code, symbol) in symbols.clone().enumerate().map(|(i, s)| (code_base + i as u16, s)) {
            literal_builder.set_mapping(bitwidth, symbol, code);
        }
    }

    let mut distance_builder = huffman::EncoderBuilder::new(30);
    for i in 0..30 {
        distance_builder.set_mapping(5, i, i);
    }

    (literal_builder.finish(), distance_builder.finish())
}

pub fn fixed_decoders() -> (huffman::Decoder, huffman::Decoder) {
    let mut literal_builder = huffman::DecoderBuilder::new(9);
    for &(bitwidth, ref symbols, code_base) in &FIXED_LITERAL_OR_LENGTH_CODE_TABLE {
        for (code, symbol) in symbols.clone().enumerate().map(|(i, s)| (code_base + i as u16, s)) {
            literal_builder.set_mapping(bitwidth, code, symbol);
        }
    }

    let mut distance_builder = huffman::DecoderBuilder::new(5);
    for i in 0..30 {
        distance_builder.set_mapping(5, i, i);
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
    let indices = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    for &i in indices.iter().take(bitwidth_code_count as usize) {
        bitwidth_code_bitwidthes[i] = try!(reader.read_bits(3)) as u8;
    }
    let mut bitwidth_decoder = huffman::DecoderBuilder::from_bitwidthes(&bitwidth_code_bitwidthes);

    let mut literal_code_bitwidthes = Vec::with_capacity(literal_code_count as usize);
    while literal_code_bitwidthes.len() < literal_code_count as usize {
        let c = try!(bitwidth_decoder.decode(reader));
        try!(load_bitwidthes(reader, c, &mut literal_code_bitwidthes));
    }

    let mut distance_code_bitwidthes = Vec::with_capacity(distance_code_count as usize);
    while distance_code_bitwidthes.len() < distance_code_count as usize {
        let c = try!(bitwidth_decoder.decode(reader));
        try!(load_bitwidthes(reader, c, &mut distance_code_bitwidthes));
    }

    Ok((huffman::DecoderBuilder::from_bitwidthes(&literal_code_bitwidthes),
        huffman::DecoderBuilder::from_bitwidthes(&distance_code_bitwidthes)))
}

fn load_bitwidthes<R>(reader: &mut bit::BitReader<R>,
                      code: u16,
                      bitwidthes: &mut Vec<u8>)
                      -> io::Result<()>
    where R: io::Read
{
    match code {
        0...15 => {
            bitwidthes.push(code as u8);
        }
        16 => {
            let count = try!(reader.read_bits(2)) + 3;
            let last = try!(bitwidthes.last()
                .cloned()
                .ok_or_else(|| invalid_data_error!("No preceeding value")));
            bitwidthes.extend(iter::repeat(last).take(count as usize));
        }
        17 => {
            let zeros = try!(reader.read_bits(3)) + 3;
            bitwidthes.extend(iter::repeat(0).take(zeros as usize));
        }
        18 => {
            let zeros = try!(reader.read_bits(7)) + 11;
            bitwidthes.extend(iter::repeat(0).take(zeros as usize));
        }
        _ => unreachable!(),
    }
    Ok(())
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

    let literal_code_count = cmp::max(257, literal_encoder.used_max_code().unwrap_or(0) + 1);
    let distance_code_count = cmp::max(1, distance_encoder.used_max_code().unwrap_or(0) + 1);

    let mut syms: Vec<Sym> = Vec::new();
    for &(e, size) in &[(&literal_encoder, literal_code_count),
                        (&distance_encoder, distance_code_count)] {
        for (i, c) in e.table.iter().take(size as usize).map(|x| x.0).enumerate() {
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
    let indices = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let bitwidth_code_count =
        cmp::max(4,
                 indices.iter()
                     .rev()
                     .position(|&i| bitwidth_encoder.table[i].0 > 0)
                     .map_or(0, |trailing_zeros| 19 - trailing_zeros)) as u16;
    try!(writer.write_bits(5, literal_code_count - 257));
    try!(writer.write_bits(5, distance_code_count - 1));
    try!(writer.write_bits(4, bitwidth_code_count - 4));
    for &i in indices.iter().take(bitwidth_code_count as usize) {
        try!(writer.write_bits(3, bitwidth_encoder.table[i].0 as u16));
    }
    for &(code, bits, extra) in &codes {
        try!(bitwidth_encoder.encode(writer, code as u16));
        if bits > 0 {
            try!(writer.write_bits(bits, extra as u16));
        }
    }
    Ok(())
}
