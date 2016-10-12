use std::io;
use std::iter;
use byteorder::LittleEndian;
use byteorder::WriteBytesExt;

use bit;
use lz77;
use lz77::Symbol;
use huffman;
use Finish;
use super::huffman_codes;

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum CompressionLevel {
    NoCompression,
    BestSpeed,
    Balance,
    BestCompression,
}
impl Default for CompressionLevel {
    fn default() -> Self {
        CompressionLevel::Balance
    }
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct EncodeOptions {
    level: CompressionLevel,
    lz77_window_size: u16,
}
impl Default for EncodeOptions {
    fn default() -> Self {
        EncodeOptions {
            level: CompressionLevel::default(),
            lz77_window_size: 0x8000,
        }
    }
}
impl EncodeOptions {
    pub fn new() -> Self {
        EncodeOptions::default()
    }
    pub fn get_level(&self) -> CompressionLevel {
        self.level.clone()
    }
    pub fn get_window_size(&self) -> u16 {
        self.lz77_window_size
    }
    pub fn level(&mut self, level: CompressionLevel) -> &mut Self {
        self.level = level;
        self
    }
    pub fn no_compression(&mut self) -> &mut Self {
        self.level(CompressionLevel::NoCompression)
    }
    pub fn best_speed(&mut self) -> &mut Self {
        self.level(CompressionLevel::BestSpeed)
    }
    pub fn best_compression(&mut self) -> &mut Self {
        self.level(CompressionLevel::BestCompression)
    }
    pub fn window_size(&mut self, size: u16) -> &mut Self {
        self.lz77_window_size = size;
        self
    }
    pub fn encoder<W>(&self, inner: W) -> Encoder<W>
        where W: io::Write
    {
        Encoder::with_options(inner, self.clone())
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
    fn new(huffman: H, options: &EncodeOptions) -> Self {
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
        try!(writer.write_bits(2, self.huffman.mode() as u16));
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
    fn mode(&self) -> u8;
    fn save_codes<W>(&mut self,
                     _writer: &mut bit::BitWriter<W>,
                     _literal_encoder: &huffman::Encoder,
                     _distance_encoder: &huffman::Encoder)
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
    fn mode(&self) -> u8 {
        0b10
    }
    fn save_codes<W>(&mut self,
                     writer: &mut bit::BitWriter<W>,
                     literal_encoder: &huffman::Encoder,
                     distance_encoder: &huffman::Encoder)
                     -> io::Result<()>
        where W: io::Write
    {
        huffman_codes::save_dynamic_codes(writer, literal_encoder, distance_encoder)
    }
}

#[derive(Debug)]
struct FixedCodes {
    literal_encoder: huffman::Encoder,
    distance_encoder: huffman::Encoder,
}
impl FixedCodes {
    fn new() -> Self {
        let (literal_encoder, distance_encoder) = huffman_codes::fixed_encoders();
        FixedCodes {
            literal_encoder: literal_encoder,
            distance_encoder: distance_encoder,
        }
    }
}
impl EncodeBlock for FixedCodes {
    fn get_encoders(&mut self, _block: &[Symbol]) -> (huffman::Encoder, huffman::Encoder) {
        (self.literal_encoder.clone(), self.distance_encoder.clone())
    }
    fn mode(&self) -> u8 {
        0b01
    }
}

#[derive(Debug)]
pub struct Encoder<W> {
    writer: bit::BitWriter<W>,
    block: Block,
    options: EncodeOptions,
}
impl<W> Encoder<W>
    where W: io::Write
{
    pub fn new(inner: W) -> Self {
        Self::with_options(inner, EncodeOptions::default())
    }
    pub fn with_options(inner: W, options: EncodeOptions) -> Self {
        let block = match options.level {
            CompressionLevel::NoCompression => Block::Raw(RawBlock::new()),
            CompressionLevel::BestSpeed => {
                Block::Fixed(HuffmanBlock::new(FixedCodes::new(), &options))
            }
            _ => Block::Dynamic(HuffmanBlock::new(DynamicCodes::new(), &options)),
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
    pub fn finish(mut self) -> Finish<W> {
        match self.block.finish(&mut self.writer) {
            Ok(_) => Finish::new(self.writer.into_inner(), None),
            Err(e) => Finish::new(self.writer.into_inner(), Some(e)),
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
