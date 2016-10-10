// https://tools.ietf.org/html/rfc1950
use std::io;
use byteorder::ReadBytesExt;
use byteorder::BigEndian;

use deflate;
use checksum;

pub const COMPRESSION_METHOD_DEFLATE: u8 = 8;

#[derive(Debug, Clone)]
pub enum CompressionLevel {
    Fastest = 0,
    Fast = 1,
    Default = 2,
    Slowest = 3,
}

#[derive(Debug, Clone)]
pub struct Header {
    lz77_window_size: u16,
    compression_level: CompressionLevel,
}
impl Header {
    pub fn lz77_window_size(&self) -> u16 {
        self.lz77_window_size
    }
    pub fn compression_level(&self) -> CompressionLevel {
        self.compression_level.clone()
    }
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        let cmf = try!(reader.read_u8());
        let flg = try!(reader.read_u8());
        let check = (cmf as u16) << 8 + flg as u16;
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
        if compression_info > 7 {
            return Err(invalid_data_error!("CINFO above 7 are not allowed: value={}",
                                           compression_info));
        }
        let lz77_window_size = 2u16.pow(compression_info as u32 + 8);

        let dict_flag = (flg & 0b10000) != 0;
        if dict_flag {
            let dictionary_id = try!(reader.read_u32::<BigEndian>());
            return Err(invalid_data_error!("Preset dictionaries are not supported: \
                                            dictionary_id=0x{:X}",
                                           dictionary_id));
        }
        let compression_level = match flg >> 5 {
            0 => CompressionLevel::Fastest,
            1 => CompressionLevel::Fast,
            2 => CompressionLevel::Default,
            3 => CompressionLevel::Slowest,
            _ => unreachable!(),
        };

        Ok(Header {
            lz77_window_size: lz77_window_size,
            compression_level: compression_level,
        })
    }
}

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
    pub fn new(mut inner: R) -> io::Result<Self> {
        let header = try!(Header::read_from(&mut inner));
        Ok(Decoder {
            header: header,
            reader: deflate::Decoder::new(inner),
            adler32: checksum::Adler32::new(),
            eos: false,
        })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn into_inner(self) -> R {
        self.reader.into_inner()
    }
}
impl<R> io::Read for Decoder<R>
    where R: io::Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.eos {
            return Ok(0);
        }

        let read_size = try!(self.reader.read(buf));
        if read_size == 0 {
            self.eos = true;
            let adler32 = try!(self.reader.as_inner_mut().read_u32::<BigEndian>());
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
