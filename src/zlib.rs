//! The encoder and decoder of the ZLIB format.
//!
//! The ZLIB format is defined in [RFC-1950](https://tools.ietf.org/html/rfc1950).
//!
//! # Examples
//! ```
//! use std::io::{self, Read};
//! use libflate::zlib::{Encoder, Decoder};
//!
//! // Encoding
//! let mut encoder = Encoder::new(Vec::new()).unwrap();
//! io::copy(&mut &b"Hello World!"[..], &mut encoder).unwrap();
//! let encoded_data = encoder.finish().into_result().unwrap();
//!
//! // Decoding
//! let mut decoder = Decoder::new(io::Cursor::new(encoded_data)).unwrap();
//! let mut decoded_data = Vec::new();
//! decoder.read_to_end(&mut decoded_data).unwrap();
//!
//! assert_eq!(decoded_data, b"Hello World!");
//! ```
use std::io;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;

use lz77;
use deflate;
use checksum;
use finish::Finish;

const COMPRESSION_METHOD_DEFLATE: u8 = 8;

/// Compression levels defined by the ZLIB format.
#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum CompressionLevel {
    /// Compressor used fastest algorithm.
    Fastest = 0,

    /// Compressor used fast algorithm.
    Fast = 1,

    /// Compressor used default algorithm.
    Default = 2,

    /// Compressor used maximum compression, slowest algorithm.
    Slowest = 3,
}
impl CompressionLevel {
    fn from_u2(level: u8) -> Self {
        match level {
            0 => CompressionLevel::Fastest,
            1 => CompressionLevel::Fast,
            2 => CompressionLevel::Default,
            3 => CompressionLevel::Slowest,
            _ => unreachable!(),
        }
    }
    fn as_u2(&self) -> u8 {
        self.clone() as u8
    }
}
impl From<lz77::CompressionLevel> for CompressionLevel {
    fn from(f: lz77::CompressionLevel) -> Self {
        match f {
            lz77::CompressionLevel::None => CompressionLevel::Fastest,
            lz77::CompressionLevel::Fast => CompressionLevel::Fast,
            lz77::CompressionLevel::Balance => CompressionLevel::Default,
            lz77::CompressionLevel::Best => CompressionLevel::Slowest,
        }
    }
}

/// LZ77 Window sizes defined by the ZLIB format.
#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum Lz77WindowSize {
    /// 256 bytes
    B256 = 0,

    /// 512 btyes
    B512 = 1,

    /// 1 kilobyte
    KB1 = 2,

    /// 2 kilobytes
    KB2 = 3,

    /// 4 kitobytes
    KB4 = 4,

    /// 8 kitobytes
    KB8 = 5,

    /// 16 kitobytes
    KB16 = 6,

    /// 32 kitobytes
    KB32 = 7,
}
impl Lz77WindowSize {
    fn from_u4(compression_info: u8) -> Option<Self> {
        match compression_info {
            0 => Some(Lz77WindowSize::B256),
            1 => Some(Lz77WindowSize::B512),
            2 => Some(Lz77WindowSize::KB1),
            3 => Some(Lz77WindowSize::KB2),
            4 => Some(Lz77WindowSize::KB4),
            5 => Some(Lz77WindowSize::KB8),
            6 => Some(Lz77WindowSize::KB16),
            7 => Some(Lz77WindowSize::KB32),
            _ => None,
        }
    }
    fn as_u4(&self) -> u8 {
        self.clone() as u8
    }

    /// Converts from `u16` to Lz77WindowSize`.
    ///
    /// Fractions are rounded to next upper window size.
    /// If `size` exceeds maximum window size,
    /// `lz77::MAX_WINDOW_SIZE` will be used instead.
    ///
    /// # Examples
    /// ```
    /// use libflate::zlib::Lz77WindowSize;
    ///
    /// assert_eq!(Lz77WindowSize::from_u16(15000), Lz77WindowSize::KB16);
    /// assert_eq!(Lz77WindowSize::from_u16(16384), Lz77WindowSize::KB16);
    /// assert_eq!(Lz77WindowSize::from_u16(16385), Lz77WindowSize::KB32);
    /// assert_eq!(Lz77WindowSize::from_u16(40000), Lz77WindowSize::KB32);
    /// ```
    pub fn from_u16(size: u16) -> Self {
        use self::Lz77WindowSize::*;
        if 16384 < size {
            KB32
        } else if 8192 < size {
            KB16
        } else if 4096 < size {
            KB8
        } else if 2048 < size {
            KB4
        } else if 1024 < size {
            KB2
        } else if 512 < size {
            KB1
        } else if 256 < size {
            B512
        } else {
            B256
        }
    }

    /// Converts from `Lz77WindowSize` to `u16`.
    ///
    /// # Examples
    /// ```
    /// use libflate::zlib::Lz77WindowSize;
    ///
    /// assert_eq!(Lz77WindowSize::KB16.to_u16(), 16384u16);
    /// ```
    pub fn to_u16(&self) -> u16 {
        use self::Lz77WindowSize::*;
        match *self {
            B256 => 256,
            B512 => 512,
            KB1 => 1024,
            KB2 => 2048,
            KB4 => 4096,
            KB8 => 8192,
            KB16 => 16384,
            KB32 => 32768,
        }
    }
}

/// ZLIB header.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Header {
    window_size: Lz77WindowSize,
    compression_level: CompressionLevel,
}
impl Header {
    /// Returns the LZ77 window size stored in the header.
    pub fn window_size(&self) -> Lz77WindowSize {
        self.window_size.clone()
    }
    /// Returns the compression level stored in the header.
    pub fn compression_level(&self) -> CompressionLevel {
        self.compression_level.clone()
    }
    fn from_lz77<E>(lz77: &E) -> Self
        where E: lz77::Lz77Encode
    {
        Header {
            compression_level: From::from(lz77.compression_level()),
            window_size: Lz77WindowSize::from_u16(lz77.window_size()),
        }
    }
    fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        let cmf = reader.read_u8()?;
        let flg = reader.read_u8()?;
        let check = ((cmf as u16) << 8) + flg as u16;
        if check % 31 != 0 {
            return Err(invalid_data_error!("Inconsistent ZLIB check bits: `CMF({}) * 256 + \
                                            FLG({})` must be a multiple of 31",
                                           cmf,
                                           flg));
        }

        let compression_method = cmf & 0b1111;
        let compression_info = cmf >> 4;
        if compression_method != COMPRESSION_METHOD_DEFLATE {
            return Err(invalid_data_error!("Compression methods other than DEFLATE(8) are \
                                            unsupported: method={}",
                                           compression_method));
        }
        let window_size = Lz77WindowSize::from_u4(compression_info)
            .ok_or_else(|| {
                            invalid_data_error!("CINFO above 7 are not allowed: value={}",
                                                compression_info)
                        })?;

        let dict_flag = (flg & 0b100000) != 0;
        if dict_flag {
            let dictionary_id = reader.read_u32::<BigEndian>()?;
            return Err(invalid_data_error!("Preset dictionaries are not supported: \
                                            dictionary_id=0x{:X}",
                                           dictionary_id));
        }
        let compression_level = CompressionLevel::from_u2(flg >> 6);
        Ok(Header {
               window_size: window_size,
               compression_level: compression_level,
           })
    }
    fn write_to<W>(&self, mut writer: W) -> io::Result<()>
        where W: io::Write
    {
        let cmf = (self.window_size.as_u4() << 4) | COMPRESSION_METHOD_DEFLATE;
        let mut flg = self.compression_level.as_u2() << 6;
        let check = ((cmf as u16) << 8) + flg as u16;
        if check % 31 != 0 {
            flg += (31 - check % 31) as u8;
        }
        writer.write_u8(cmf)?;
        writer.write_u8(flg)?;
        Ok(())
    }
}

/// ZLIB decoder.
#[derive(Debug)]
pub struct Decoder<R> {
    header: Header,
    reader: deflate::Decoder<R>,
    adler32: checksum::Adler32,
    eos: bool,
}
impl<R> Decoder<R>
    where R: io::Read
{
    /// Makes a new decoder instance.
    ///
    /// `inner` is to be decoded ZLIB stream.
    ///
    /// # Examples
    /// ```
    /// use std::io::{Cursor, Read};
    /// use libflate::zlib::Decoder;
    ///
    /// let encoded_data = [120, 156, 243, 72, 205, 201, 201, 87, 8, 207, 47,
    ///                     202, 73, 81, 4, 0, 28, 73, 4, 62];
    ///
    /// let mut decoder = Decoder::new(Cursor::new(&encoded_data)).unwrap();
    /// let mut buf = Vec::new();
    /// decoder.read_to_end(&mut buf).unwrap();
    ///
    /// assert_eq!(buf, b"Hello World!");
    /// ```
    pub fn new(mut inner: R) -> io::Result<Self> {
        let header = Header::read_from(&mut inner)?;
        Ok(Decoder {
               header: header,
               reader: deflate::Decoder::new(inner),
               adler32: checksum::Adler32::new(),
               eos: false,
           })
    }

    /// Returns the header of the ZLIB stream.
    ///
    /// # Examples
    /// ```
    /// use std::io::Cursor;
    /// use libflate::zlib::{Decoder, CompressionLevel};
    ///
    /// let encoded_data = [120, 156, 243, 72, 205, 201, 201, 87, 8, 207, 47,
    ///                     202, 73, 81, 4, 0, 28, 73, 4, 62];
    ///
    /// let decoder = Decoder::new(Cursor::new(&encoded_data)).unwrap();
    /// assert_eq!(decoder.header().compression_level(),
    ///            CompressionLevel::Default);
    /// ```
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Unwraps this `Decoder`, returning the underlying reader.
    ///
    /// # Examples
    /// ```
    /// use std::io::Cursor;
    /// use libflate::zlib::Decoder;
    ///
    /// let encoded_data = [120, 156, 243, 72, 205, 201, 201, 87, 8, 207, 47,
    ///                     202, 73, 81, 4, 0, 28, 73, 4, 62];
    ///
    /// let decoder = Decoder::new(Cursor::new(&encoded_data)).unwrap();
    /// assert_eq!(decoder.into_inner().into_inner(), &encoded_data);
    /// ```
    pub fn into_inner(self) -> R {
        self.reader.into_inner()
    }
}
impl<R> io::Read for Decoder<R>
    where R: io::Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.eos {
            Ok(0)
        } else {
            let read_size = self.reader.read(buf)?;
            if read_size == 0 {
                self.eos = true;
                let adler32 = self.reader.as_inner_mut().read_u32::<BigEndian>()?;
                if adler32 != self.adler32.value() {
                    Err(invalid_data_error!("Adler32 checksum mismatched: value={}, expected={}",
                                            self.adler32.value(),
                                            adler32))
                } else {
                    Ok(0)
                }
            } else {
                self.adler32.update(&buf[..read_size]);
                Ok(read_size)
            }
        }
    }
}

/// Options for a ZLIB encoder.
#[derive(Debug)]
pub struct EncodeOptions<E>
    where E: lz77::Lz77Encode
{
    header: Header,
    options: deflate::EncodeOptions<E>,
}
impl Default for EncodeOptions<lz77::DefaultLz77Encoder> {
    fn default() -> Self {
        EncodeOptions {
            header: Header::from_lz77(&lz77::DefaultLz77Encoder::new()),
            options: Default::default(),
        }
    }
}
impl EncodeOptions<lz77::DefaultLz77Encoder> {
    /// Makes a default instance.
    ///
    /// # Examples
    /// ```
    /// use libflate::zlib::{Encoder, EncodeOptions};
    ///
    /// let options = EncodeOptions::new();
    /// let encoder = Encoder::with_options(Vec::new(), options).unwrap();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }
}
impl<E> EncodeOptions<E>
    where E: lz77::Lz77Encode
{
    /// Specifies the LZ77 encoder used to compress input data.
    ///
    /// # Example
    /// ```
    /// use libflate::lz77::DefaultLz77Encoder;
    /// use libflate::zlib::{Encoder, EncodeOptions};
    ///
    /// let options = EncodeOptions::with_lz77(DefaultLz77Encoder::new());
    /// let encoder = Encoder::with_options(Vec::new(), options).unwrap();
    /// ```
    pub fn with_lz77(lz77: E) -> Self {
        EncodeOptions {
            header: Header::from_lz77(&lz77),
            options: deflate::EncodeOptions::with_lz77(lz77),
        }
    }

    /// Disables LZ77 compression.
    ///
    /// # Example
    /// ```
    /// use libflate::lz77::DefaultLz77Encoder;
    /// use libflate::zlib::{Encoder, EncodeOptions};
    ///
    /// let options = EncodeOptions::new().no_compression();
    /// let encoder = Encoder::with_options(Vec::new(), options).unwrap();
    /// ```
    pub fn no_compression(mut self) -> Self {
        self.options = self.options.no_compression();
        self.header.compression_level = CompressionLevel::Fastest;
        self
    }

    /// Specifies the hint of the size of a DEFLATE block.
    ///
    /// The default value is `deflate::DEFAULT_BLOCK_SIZE`.
    ///
    /// # Example
    /// ```
    /// use libflate::zlib::{Encoder, EncodeOptions};
    ///
    /// let options = EncodeOptions::new().block_size(512 * 1024);
    /// let encoder = Encoder::with_options(Vec::new(), options).unwrap();
    /// ```
    pub fn block_size(mut self, size: usize) -> Self {
        self.options = self.options.block_size(size);
        self
    }

    /// Specifies to compress with fixed huffman codes.
    ///
    /// # Example
    /// ```
    /// use libflate::zlib::{Encoder, EncodeOptions};
    ///
    /// let options = EncodeOptions::new().fixed_huffman_codes();
    /// let encoder = Encoder::with_options(Vec::new(), options).unwrap();
    /// ```
    pub fn fixed_huffman_codes(mut self) -> Self {
        self.options = self.options.fixed_huffman_codes();
        self
    }
}

/// ZLIB encoder.
#[derive(Debug)]
pub struct Encoder<W, E = lz77::DefaultLz77Encoder> {
    header: Header,
    writer: deflate::Encoder<W, E>,
    adler32: checksum::Adler32,
}
impl<W> Encoder<W, lz77::DefaultLz77Encoder>
    where W: io::Write
{
    /// Makes a new encoder instance.
    ///
    /// Encoded ZLIB stream is written to `inner`.
    ///
    /// # Examples
    /// ```
    /// use std::io::Write;
    /// use libflate::zlib::Encoder;
    ///
    /// let mut encoder = Encoder::new(Vec::new()).unwrap();
    /// encoder.write_all(b"Hello World!").unwrap();
    ///
    /// assert_eq!(encoder.finish().into_result().unwrap(),
    ///            [120, 156, 5, 128, 65, 9, 0, 0, 8, 3, 171, 104, 27, 27, 88, 64, 127,
    ///             7, 131, 245, 127, 140, 121, 80, 173, 204, 117, 0, 28, 73, 4, 62]);
    /// ```
    pub fn new(inner: W) -> io::Result<Self> {
        Self::with_options(inner, EncodeOptions::default())
    }
}
impl<W, E> Encoder<W, E>
    where W: io::Write,
          E: lz77::Lz77Encode
{
    /// Makes a new encoder instance with specified options.
    ///
    /// Encoded ZLIB stream is written to `inner`.
    ///
    /// # Examples
    /// ```
    /// use std::io::Write;
    /// use libflate::zlib::{Encoder, EncodeOptions};
    ///
    /// let options = EncodeOptions::new().no_compression();
    /// let mut encoder = Encoder::with_options(Vec::new(), options).unwrap();
    /// encoder.write_all(b"Hello World!").unwrap();
    ///
    /// assert_eq!(encoder.finish().into_result().unwrap(),
    ///            [120, 1, 1, 12, 0, 243, 255, 72, 101, 108, 108, 111, 32, 87, 111,
    ///             114, 108, 100, 33, 28, 73, 4, 62]);
    /// ```
    pub fn with_options(mut inner: W, options: EncodeOptions<E>) -> io::Result<Self> {
        options.header.write_to(&mut inner)?;
        Ok(Encoder {
               header: options.header,
               writer: deflate::Encoder::with_options(inner, options.options),
               adler32: checksum::Adler32::new(),
           })
    }

    /// Returns the header of the ZLIB stream.
    ///
    /// # Examples
    /// ```
    /// use libflate::zlib::{Encoder, Lz77WindowSize};
    ///
    /// let encoder = Encoder::new(Vec::new()).unwrap();
    /// assert_eq!(encoder.header().window_size(), Lz77WindowSize::KB32);
    /// ```
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Writes the ZLIB trailer and returns the inner stream.
    ///
    /// # Examples
    /// ```
    /// use std::io::Write;
    /// use libflate::zlib::Encoder;
    ///
    /// let mut encoder = Encoder::new(Vec::new()).unwrap();
    /// encoder.write_all(b"Hello World!").unwrap();
    ///
    /// assert_eq!(encoder.finish().into_result().unwrap(),
    ///            [120, 156, 5, 128, 65, 9, 0, 0, 8, 3, 171, 104, 27, 27, 88, 64, 127,
    ///             7, 131, 245, 127, 140, 121, 80, 173, 204, 117, 0, 28, 73, 4, 62]);
    /// ```
    pub fn finish(self) -> Finish<W, io::Error> {
        let mut inner = finish_try!(self.writer.finish());
        match inner
                  .write_u32::<BigEndian>(self.adler32.value())
                  .and_then(|_| inner.flush()) {
            Ok(_) => Finish::new(inner, None),
            Err(e) => Finish::new(inner, Some(e)),
        }
    }
}
impl<W> io::Write for Encoder<W>
    where W: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written_size = self.writer.write(buf)?;
        self.adler32.update(&buf[..written_size]);
        Ok(written_size)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod test {
    use std::io;
    use super::*;

    fn decode_all(buf: &[u8]) -> io::Result<Vec<u8>> {
        let mut decoder = Decoder::new(io::Cursor::new(buf)).unwrap();
        let mut buf = Vec::with_capacity(buf.len());
        io::copy(&mut decoder, &mut buf)?;
        Ok(buf)
    }
    fn default_encode(buf: &[u8]) -> io::Result<Vec<u8>> {
        let mut encoder = Encoder::new(Vec::new()).unwrap();
        io::copy(&mut &buf[..], &mut encoder).unwrap();
        encoder.finish().into_result()
    }
    macro_rules! assert_encode_decode {
        ($input:expr) => {
            {
                let encoded = default_encode(&$input[..]).unwrap();
                assert_eq!(decode_all(&encoded).unwrap(), &$input[..]);
            }
        }
    }

    #[test]
    fn decode_works() {
        let encoded = [120, 156, 243, 72, 205, 201, 201, 87, 8, 207, 47, 202, 73, 81, 4, 0, 28,
                       73, 4, 62];
        let mut decoder = Decoder::new(io::Cursor::new(&encoded)).unwrap();
        assert_eq!(*decoder.header(),
                   Header {
                       window_size: Lz77WindowSize::KB32,
                       compression_level: CompressionLevel::Default,
                   });

        let mut buf = Vec::new();
        io::copy(&mut decoder, &mut buf).unwrap();

        let expected = b"Hello World!";
        assert_eq!(buf, expected);
    }

    #[test]
    fn default_encode_works() {
        let plain = b"Hello World! Hello ZLIB!!";
        let mut encoder = Encoder::new(Vec::new()).unwrap();
        io::copy(&mut &plain[..], &mut encoder).unwrap();
        let encoded = encoder.finish().into_result().unwrap();
        assert_eq!(decode_all(&encoded).unwrap(), plain);
    }

    #[test]
    fn best_speed_encode_works() {
        let plain = b"Hello World! Hello ZLIB!!";
        let mut encoder = Encoder::with_options(Vec::new(),
                                                EncodeOptions::default().fixed_huffman_codes())
                .unwrap();
        io::copy(&mut &plain[..], &mut encoder).unwrap();
        let encoded = encoder.finish().into_result().unwrap();
        assert_eq!(decode_all(&encoded).unwrap(), plain);
    }

    #[test]
    fn raw_encode_works() {
        let plain = b"Hello World!";
        let mut encoder = Encoder::with_options(Vec::new(), EncodeOptions::new().no_compression())
            .unwrap();
        io::copy(&mut &plain[..], &mut encoder).unwrap();
        let encoded = encoder.finish().into_result().unwrap();
        let expected = [120, 1, 1, 12, 0, 243, 255, 72, 101, 108, 108, 111, 32, 87, 111, 114, 108,
                        100, 33, 28, 73, 4, 62];
        assert_eq!(encoded, expected);
        assert_eq!(decode_all(&encoded).unwrap(), plain);
    }

    #[test]
    fn test_issue_2() {
        // See: https://github.com/sile/libflate/issues/2
        assert_encode_decode!([163, 181, 167, 40, 62, 239, 41, 125, 189, 217, 61, 122, 20, 136,
                               160, 178, 119, 217, 41, 125, 189, 97, 195, 101, 47, 170]);
        assert_encode_decode!([162, 58, 99, 211, 7, 64, 96, 36, 57, 155, 53, 166, 76, 14, 238,
                               66, 148, 154, 124, 162, 58, 99, 188, 138, 131, 171, 189, 54, 229,
                               192, 38, 29, 240, 122, 28]);
        assert_encode_decode!([239, 238, 212, 42, 5, 46, 186, 67, 122, 247, 30, 61, 219, 62, 228,
                               202, 164, 205, 139, 109, 99, 181, 99, 181, 99, 122, 30, 12, 62,
                               46, 27, 145, 241, 183, 137]);
        assert_encode_decode!([88, 202, 64, 12, 125, 108, 153, 49, 164, 250, 71, 19, 4, 108, 111,
                               108, 237, 205, 208, 77, 217, 100, 118, 49, 10, 64, 12, 125, 51,
                               202, 69, 67, 181, 146, 86]);
    }
}
