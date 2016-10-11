use std::io;
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
    Static(StaticHuffmanBlock),
    Dynamic(DynamicHuffmanBlock),
}
impl Block {
    fn write<W>(&mut self, writer: &mut bit::BitWriter<W>, buf: &[u8]) -> io::Result<()>
        where W: io::Write
    {
        match *self {
            Block::Raw(ref mut b) => b.write(writer, buf),
            Block::Static(ref mut b) => b.write(writer, buf),
            Block::Dynamic(ref mut b) => b.write(writer, buf),
        }
    }
    fn finish<W>(self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        match self {
            Block::Raw(b) => b.finish(writer),
            Block::Static(b) => b.finish(writer),
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
struct StaticHuffmanBlock {
    lz77_buf: lz77::Encoder,
    huffman: SymbolEncoder,
}
impl StaticHuffmanBlock {
    fn new(options: &Options) -> Self {
        StaticHuffmanBlock {
            lz77_buf: lz77::Encoder::new(options.lz77_window_size),
            huffman: SymbolEncoder::new_fixed(),
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
        try!(self.huffman.encode(writer, &self.lz77_buf.as_slice()[..size]));
        try!(self.huffman.encode(writer, &[Symbol::EndOfBlock]));
        Ok(())
    }
}

#[derive(Debug)]
struct DynamicHuffmanBlock;
impl DynamicHuffmanBlock {
    fn new() -> Self {
        DynamicHuffmanBlock
    }
    fn write<W>(&mut self, writer: &mut bit::BitWriter<W>, buf: &[u8]) -> io::Result<()>
        where W: io::Write
    {
        panic!()
    }
    fn finish<W>(self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        panic!()
    }
}

#[derive(Debug)]
struct SymbolEncoder {
    literal_encoder: huffman::Encoder,
    distance_encoder: huffman::Encoder,
}
impl SymbolEncoder {
    fn new_fixed() -> Self {
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

        SymbolEncoder {
            literal_encoder: literal_builder.finish(),
            distance_encoder: distance_builder.finish(),
        }
    }
    pub fn encode<W>(&mut self,
                     writer: &mut bit::BitWriter<W>,
                     symbols: &[Symbol])
                     -> io::Result<()>
        where W: io::Write
    {
        for s in symbols {
            try!(self.literal_encoder.encode(writer, s.code()));
            if let Some((bits, extra)) = s.extra_lengh() {
                try!(writer.write_bits(bits, extra));
            }
            if let Some((code, bits, extra)) = s.distance() {
                try!(self.distance_encoder.encode(writer, code as u16));
                if bits > 0 {
                    try!(writer.write_bits(bits, extra));
                }
            }
        }
        Ok(())
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
            Level::BestSpeed => Block::Static(StaticHuffmanBlock::new(&options)),
            Level::Default | Level::BestCompression => Block::Dynamic(DynamicHuffmanBlock::new()),
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
