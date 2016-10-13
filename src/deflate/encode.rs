use std::io;
use std::cmp;
use byteorder::LittleEndian;
use byteorder::WriteBytesExt;

use bit;
use lz77;
use finish::Finish;
use super::codes;
use super::Symbol;
use super::BlockType;

pub const DEFAULT_BLOCK_SIZE: usize = 1024 * 1024;
const MAX_NON_COMPRESSED_BLOCK_SIZE: usize = 0xFFFF;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EncodeOptions<E = lz77::DefaultEncoder> {
    block_size: usize, // XXX:
    with_dynamic_huffman: bool,
    lz77: Option<E>,
}
impl Default for EncodeOptions<lz77::DefaultEncoder> {
    fn default() -> Self {
        Self::new()
    }
}
impl EncodeOptions<lz77::DefaultEncoder> {
    pub fn new() -> Self {
        EncodeOptions {
            block_size: DEFAULT_BLOCK_SIZE,
            with_dynamic_huffman: true,
            lz77: Some(lz77::DefaultEncoder),
        }
    }
    pub fn no_compression(mut self) -> Self {
        self.lz77 = None;
        self
    }
}
impl<E> EncodeOptions<E>
    where E: lz77::Encode
{
    pub fn with_lz77(lz77: E) -> Self {
        EncodeOptions {
            block_size: DEFAULT_BLOCK_SIZE,
            with_dynamic_huffman: true,
            lz77: Some(lz77),
        }
    }
    pub fn block_size(mut self, size: usize) -> Self {
        self.block_size = size;
        self
    }
    pub fn dynamic_huffman_codes(mut self) -> Self {
        self.with_dynamic_huffman = true;
        self
    }
    pub fn fixed_huffman_codes(mut self) -> Self {
        self.with_dynamic_huffman = false;
        self
    }
    fn get_block_type(&self) -> BlockType {
        if self.lz77.is_none() {
            BlockType::Raw
        } else if self.with_dynamic_huffman {
            BlockType::Dynamic
        } else {
            BlockType::Fixed
        }
    }
    fn get_block_size(&self) -> usize {
        if self.lz77.is_none() {
            cmp::min(self.block_size, MAX_NON_COMPRESSED_BLOCK_SIZE)
        } else {
            self.block_size
        }
    }
}

#[derive(Debug)]
pub struct Encoder<W, E = lz77::DefaultEncoder> {
    writer: bit::BitWriter<W>,
    block: Block<E>,
}
impl<W> Encoder<W, lz77::DefaultEncoder>
    where W: io::Write
{
    pub fn new(inner: W) -> Self {
        Self::with_options(inner, EncodeOptions::default())
    }
}
impl<W, E> Encoder<W, E>
    where W: io::Write,
          E: lz77::Encode
{
    pub fn with_options(inner: W, options: EncodeOptions<E>) -> Self {
        Encoder {
            writer: bit::BitWriter::new(inner),
            block: Block::new(options),
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
impl<W, E> io::Write for Encoder<W, E>
    where W: io::Write,
          E: lz77::Encode
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        try!(self.block.write(&mut self.writer, buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.as_inner_mut().flush()
    }
}

#[derive(Debug)]
struct Block<E> {
    block_type: BlockType,
    block_size: usize,
    block_buf: BlockBuf<E>,
}
impl<E> Block<E>
    where E: lz77::Encode
{
    fn new(options: EncodeOptions<E>) -> Self {
        Block {
            block_type: options.get_block_type(),
            block_size: options.get_block_size(),
            block_buf: BlockBuf::new(options.lz77, options.with_dynamic_huffman),
        }
    }
    fn write<W>(&mut self, writer: &mut bit::BitWriter<W>, buf: &[u8]) -> io::Result<()>
        where W: io::Write
    {
        self.block_buf.append(buf);
        while self.block_buf.len() >= self.block_size {
            try!(writer.write_bit(false));
            try!(writer.write_bits(2, self.block_type as u16));
            try!(self.block_buf.flush(writer));
        }
        Ok(())
    }
    fn finish<W>(mut self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        try!(writer.write_bit(true));
        try!(writer.write_bits(2, self.block_type as u16));
        try!(self.block_buf.flush(writer));
        try!(writer.flush());
        Ok(())
    }
}

#[derive(Debug)]
enum BlockBuf<E> {
    Raw(RawBuf),
    Fixed(CompressBuf<codes::Fixed, E>),
    Dynamic(CompressBuf<codes::Dynamic, E>),
}
impl<E> BlockBuf<E>
    where E: lz77::Encode
{
    fn new(lz77: Option<E>, dynamic: bool) -> Self {
        if let Some(lz77) = lz77 {
            if dynamic {
                BlockBuf::Dynamic(CompressBuf::new(codes::Dynamic, lz77))
            } else {
                BlockBuf::Fixed(CompressBuf::new(codes::Fixed, lz77))
            }
        } else {
            BlockBuf::Raw(RawBuf::new())
        }
    }
    fn append(&mut self, buf: &[u8]) {
        match *self {
            BlockBuf::Raw(ref mut b) => b.append(buf),
            BlockBuf::Fixed(ref mut b) => b.append(buf),
            BlockBuf::Dynamic(ref mut b) => b.append(buf),
        }
    }
    fn len(&self) -> usize {
        match *self {
            BlockBuf::Raw(ref b) => b.len(),
            BlockBuf::Fixed(ref b) => b.len(),
            BlockBuf::Dynamic(ref b) => b.len(),
        }
    }
    fn flush<W>(&mut self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        match *self {
            BlockBuf::Raw(ref mut b) => b.flush(writer),
            BlockBuf::Fixed(ref mut b) => b.flush(writer),
            BlockBuf::Dynamic(ref mut b) => b.flush(writer),
        }
    }
}

#[derive(Debug)]
struct RawBuf {
    buf: Vec<u8>,
}
impl RawBuf {
    fn new() -> Self {
        RawBuf { buf: Vec::new() }
    }
    fn append(&mut self, buf: &[u8]) {
        self.buf.extend(buf);
    }
    fn len(&self) -> usize {
        self.buf.len()
    }
    fn flush<W>(&mut self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        let size = cmp::min(self.buf.len(), MAX_NON_COMPRESSED_BLOCK_SIZE);
        try!(writer.flush());
        try!(writer.as_inner_mut().write_u16::<LittleEndian>(size as u16));
        try!(writer.as_inner_mut().write_u16::<LittleEndian>(!size as u16));
        try!(writer.as_inner_mut().write_all(&self.buf[..size]));
        self.buf.drain(0..size);
        Ok(())
    }
}

#[derive(Debug)]
struct CompressBuf<H, E> {
    huffman: H,
    lz77: E,
    buf: Vec<Symbol>,
    original_size: usize,
}
impl<H, E> CompressBuf<H, E>
    where H: codes::Factory,
          E: lz77::Encode
{
    fn new(huffman: H, lz77: E) -> Self {
        CompressBuf {
            huffman: huffman,
            lz77: lz77,
            buf: Vec::new(),
            original_size: 0,
        }
    }
    fn append(&mut self, buf: &[u8]) {
        self.original_size += buf.len();
        self.lz77.encode(buf, Symbol::from, &mut self.buf);
    }
    fn len(&self) -> usize {
        self.original_size
    }
    fn flush<W>(&mut self, writer: &mut bit::BitWriter<W>) -> io::Result<()>
        where W: io::Write
    {
        self.buf.push(Symbol::EndOfBlock);
        let mut codes = self.huffman.build_codes(&self.buf);
        try!(self.huffman.save(writer, &codes));
        for s in self.buf.drain(..) {
            try!(codes.literal.encode(writer, s.code()));
            if let Some((bits, extra)) = s.extra_lengh() {
                try!(writer.write_bits(bits, extra));
            }
            if let Some((code, bits, extra)) = s.distance() {
                try!(codes.distance.encode(writer, code as u16));
                if bits > 0 {
                    try!(writer.write_bits(bits, extra));
                }
            }
        }
        self.original_size = 0;
        Ok(())
    }
}
