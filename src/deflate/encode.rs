use std::io;
use std::cmp;
use std::iter;
use byteorder::LittleEndian;
use byteorder::WriteBytesExt;

use bit;
use lz77;
use huffman;
use super::Symbol;

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum Level {
    NoCompression,
    BestSpeed,
    Default,
    BestCompression,
}
impl Default for Level {
    fn default() -> Self {
        Level::Default
    }
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct Options {
    pub level: Level,
    pub lz77_window_size: u16,
}
impl Default for Options {
    fn default() -> Self {
        Options {
            level: Level::default(),
            lz77_window_size: 0x8000,
        }
    }
}
impl Options {
    pub fn new() -> Self {
        Options::default()
    }
}

#[derive(Debug)]
enum Block {
    Raw(RawBlock),
    Fixed(HuffmanBlock<FixedCodes>),
    Dynamic(HuffmanBlock<DynamicCodes>),
}
impl Block {
    fn write<W>(&mut self, writer: &mut bit::BitWriter<W>, buf: &[u8]) -> io::Result<()>
        where W: io::Write
    {
        match *self {
            Block::Raw(ref mut b) => b.write(writer, buf),
            Block::Fixed(ref mut b) => b.write(writer, buf),
            Block::Dynamic(ref mut b) => b.write(writer, buf),
        }
    }
    fn finish<W>(self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        match self {
            Block::Raw(b) => b.finish(writer),
            Block::Fixed(b) => b.finish(writer),
            Block::Dynamic(b) => b.finish(writer),
        }
    }
}

const MAX_NON_COMPRESSED_BLOCK_SIZE: usize = 0xFFFF;

#[derive(Debug)]
struct RawBlock {
    buf: Vec<u8>,
}
impl RawBlock {
    fn new() -> Self {
        RawBlock { buf: Vec::new() }
    }
    fn write<W>(&mut self, writer: &mut bit::BitWriter<W>, buf: &[u8]) -> io::Result<()>
        where W: io::Write
    {
        self.buf.extend_from_slice(buf);

        let mut start = 0;
        let mut end = MAX_NON_COMPRESSED_BLOCK_SIZE;
        while self.buf.len() >= end {
            try!(Self::write_block(writer, &self.buf[start..end], false));
            start = end;
            end += MAX_NON_COMPRESSED_BLOCK_SIZE;
        }
        self.buf.drain(..start);
        Ok(())
    }
    fn finish<W>(self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        Self::write_block(writer, &self.buf, true)
    }
    fn write_block<W>(writer: &mut bit::BitWriter<W>, buf: &[u8], is_last: bool) -> io::Result<()>
        where W: io::Write
    {
        debug_assert!(buf.len() < 0x10000);

        try!(writer.write_bit(is_last));
        try!(writer.write_bits(2, 0b00));
        try!(writer.flush());
        try!(writer.as_inner_mut().write_u16::<LittleEndian>(buf.len() as u16));
        try!(writer.as_inner_mut().write_u16::<LittleEndian>(!buf.len() as u16));
        try!(writer.as_inner_mut().write_all(buf));
        Ok(())
    }
}

// TODO: CompressionBlock?
#[derive(Debug)]
struct HuffmanBlock<H> {
    lz77_buf: lz77::Encoder,
    huffman: H,
}
impl<H> HuffmanBlock<H>
    where H: EncodeBlock
{
    fn new(huffman: H, options: &Options) -> Self {
        HuffmanBlock {
            lz77_buf: lz77::Encoder::new(options.lz77_window_size),
            huffman: huffman,
        }
    }
    fn write<W>(&mut self, writer: &mut bit::BitWriter<W>, buf: &[u8]) -> io::Result<()>
        where W: io::Write
    {
        self.lz77_buf.extend(buf);

        // TODO: parameterize
        let block_size = 0x10000;
        while self.lz77_buf.len() >= block_size {
            try!(self.write_block(writer, block_size, false));
            self.lz77_buf.drop(block_size);
        }
        Ok(())
    }
    fn finish<W>(mut self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        self.lz77_buf.flush();
        let size = self.lz77_buf.len();
        try!(self.write_block(writer, size, true));
        try!(writer.flush());
        Ok(())
    }
    fn write_block<W>(&mut self,
                      writer: &mut bit::BitWriter<W>,
                      size: usize,
                      is_last: bool)
                      -> io::Result<()>
        where W: io::Write
    {
        try!(writer.write_bit(is_last));
        try!(writer.write_bits(2, 0b01));
        try!(self.huffman.encode_block(writer, &self.lz77_buf.as_slice()[..size]));
        Ok(())
    }
}

trait EncodeBlock {
    fn encode_block<W>(&mut self,
                       writer: &mut bit::BitWriter<W>,
                       block: &[Symbol])
                       -> io::Result<()>
        where W: io::Write
    {
        let (mut literal_encoder, mut distance_encoder) = self.get_encoders(block);
        try!(self.save_codes(writer, &literal_encoder, &distance_encoder));
        for s in block.iter().chain(iter::once(&Symbol::EndOfBlock)) {
            try!(literal_encoder.encode(writer, s.code()));
            if let Some((bits, extra)) = s.extra_lengh() {
                try!(writer.write_bits(bits, extra));
            }
            if let Some((code, bits, extra)) = s.distance() {
                try!(distance_encoder.encode(writer, code as u16));
                if bits > 0 {
                    try!(writer.write_bits(bits, extra));
                }
            }
        }
        Ok(())
    }
    fn save_codes<W>(&mut self,
                     writer: &mut bit::BitWriter<W>,
                     literal_encoder: &huffman::Encoder,
                     distance_encoder: &huffman::Encoder)
                     -> io::Result<()>
        where W: io::Write
    {
        Ok(())
    }
    fn get_encoders(&mut self, block: &[Symbol]) -> (huffman::Encoder, huffman::Encoder);
}

#[derive(Debug)]
struct DynamicCodes;
impl DynamicCodes {
    fn new() -> Self {
        DynamicCodes
    }
}
impl EncodeBlock for DynamicCodes {
    fn get_encoders(&mut self, block: &[Symbol]) -> (huffman::Encoder, huffman::Encoder) {
        let mut literal_counts = [0; 286];
        let mut distance_counts = [0; 30];
        for s in block.iter().chain(iter::once(&Symbol::EndOfBlock)) {
            literal_counts[s.code() as usize] += 1;
            if let Some((d, _, _)) = s.distance() {
                distance_counts[d as usize] += 1;
            }
        }
        (huffman::EncoderBuilder::from_frequencies(&literal_counts, 15),
         huffman::EncoderBuilder::from_frequencies(&distance_counts, 15))
    }
    fn save_codes<W>(&mut self,
                     writer: &mut bit::BitWriter<W>,
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
}

#[derive(Debug)]
struct FixedCodes {
    literal_encoder: huffman::Encoder,
    distance_encoder: huffman::Encoder,
}
impl FixedCodes {
    fn new() -> Self {
        let mut literal_builder = huffman::EncoderBuilder::new(287);
        for i in 0..144 {
            literal_builder.set_mapping(8, i, 0b0011_0000 + i);
        }
        for i in 144..256 {
            literal_builder.set_mapping(9, i, 0b1_1001_0000 + i - 144);
        }
        for i in 256..280 {
            literal_builder.set_mapping(7, i, 0b000_0000 + i - 256);
        }
        for i in 280..287 {
            literal_builder.set_mapping(8, i, 0b1100_0000 + i - 280);
        }

        let mut distance_builder = huffman::EncoderBuilder::new(30);
        for i in 0..30 {
            distance_builder.set_mapping(5, i, i);
        }

        FixedCodes {
            literal_encoder: literal_builder.finish(),
            distance_encoder: distance_builder.finish(),
        }
    }
}
impl EncodeBlock for FixedCodes {
    fn get_encoders(&mut self, _block: &[Symbol]) -> (huffman::Encoder, huffman::Encoder) {
        (self.literal_encoder.clone(), self.distance_encoder.clone())
    }
}

#[derive(Debug)]
pub struct Encoder<W> {
    writer: bit::BitWriter<W>,
    block: Block,
    options: Options,
}
impl<W> Encoder<W>
    where W: io::Write
{
    pub fn new(inner: W) -> Self {
        Self::with_options(inner, Options::default())
    }
    pub fn with_options(inner: W, options: Options) -> Self {
        let block = match options.level {
            Level::NoCompression => Block::Raw(RawBlock::new()),
            Level::BestSpeed => Block::Fixed(HuffmanBlock::new(FixedCodes::new(), &options)),
            Level::Default | Level::BestCompression => {
                Block::Dynamic(HuffmanBlock::new(DynamicCodes::new(), &options))
            }
        };
        Encoder {
            writer: bit::BitWriter::new(inner),
            block: block,
            options: options,
        }
    }
    pub fn as_inner_ref(&self) -> &W {
        self.writer.as_inner_ref()
    }
    pub fn as_inner_mut(&mut self) -> &mut W {
        self.writer.as_inner_mut()
    }
    pub fn into_inner(self) -> W {
        self.writer.into_inner()
    }
    pub fn finish(mut self) -> Result<W, (W, io::Error)> {
        match self.block.finish(&mut self.writer) {
            Ok(_) => Ok(self.writer.into_inner()),
            Err(e) => Err((self.writer.into_inner(), e)),
        }
    }
}
impl<W> io::Write for Encoder<W>
    where W: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        try!(self.block.write(&mut self.writer, buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.as_inner_mut().flush()
    }
}
