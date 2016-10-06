/// https://tools.ietf.org/html/rfc1952
use std::io;
use std::io::Read;
use std::ffi::CString;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;

use deflate;

pub const GZIP_ID: [u8; 2] = [31, 139];

pub const COMPRESSION_METHOD_DEFLATE: u8 = 8;

pub const OS_FAT: u8 = 0;
pub const OS_AMIGA: u8 = 1;
pub const OS_VMS: u8 = 2;
pub const OS_UNIX: u8 = 3;
pub const OS_VM_CMS: u8 = 4;
pub const OS_ATARI_TOS: u8 = 5;
pub const OS_HPFS: u8 = 6;
pub const OS_MACINTOSH: u8 = 7;
pub const OS_Z_SYSTEM: u8 = 8;
pub const OS_CPM: u8 = 9;
pub const OS_TOPS20: u8 = 10;
pub const OS_NTFS: u8 = 11;
pub const OS_QDOS: u8 = 12;
pub const OS_ACORN_RISCOS: u8 = 13;
pub const OS_UNKNOWN: u8 = 255;

#[derive(Debug, Clone)]
pub enum CompressionMethod {
    Deflate,
    Undefined(u8),
}
impl CompressionMethod {
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        Ok(match try!(reader.read_u8()) {
            COMPRESSION_METHOD_DEFLATE => CompressionMethod::Deflate,
            method => CompressionMethod::Undefined(method),
        })
    }
}

bitflags! {
    pub flags Flags: u8 {
        const F_TEXT    = 0b000001,
        const F_HCRC    = 0b000010,
        const F_EXTRA   = 0b000100,
        const F_NAME    = 0b001000,
        const F_COMMENT = 0b010000,
    }
}

#[derive(Debug, Clone)]
pub struct Trailer {
    pub crc: u32,
    pub input_size: u32,
}
impl Trailer {
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        Ok(Trailer {
            crc: try!(reader.read_u32::<LittleEndian>()),
            input_size: try!(reader.read_u32::<LittleEndian>()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub id: [u8; 2],
    pub compression_method: CompressionMethod,
    pub flags: Flags,
    pub modification_time: u32,
    pub extra_flags: u8,
    pub os: Os,
    pub extra_field: Option<ExtraField>,
    pub filename: Option<CString>,
    pub comment: Option<CString>,
    pub header_crc: Option<u16>,
}
impl Default for Header {
    fn default() -> Self {
        Header {
            id: GZIP_ID,
            compression_method: CompressionMethod::Deflate,
            flags: Flags::empty(),
            modification_time: 0,
            extra_flags: 0,
            os: Os::Unix,
            extra_field: None,
            filename: None,
            comment: None,
            header_crc: None,
        }
    }
}
impl Header {
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        let mut this = Header::default();
        try!(reader.read_exact(&mut this.id));
        if this.id != GZIP_ID {
            return invalid_data_error(&format!("Unexpected GZIP ID: value={:?}, expected={:?}",
                                               this.id,
                                               GZIP_ID));
        }
        this.compression_method = try!(CompressionMethod::read_from(&mut reader));
        this.flags = Flags::from_bits_truncate(try!(reader.read_u8()));
        this.modification_time = try!(reader.read_u32::<LittleEndian>());
        this.extra_flags = try!(reader.read_u8());
        this.os = try!(Os::read_from(&mut reader));
        if this.flags.contains(F_EXTRA) {
            this.extra_field = Some(try!(ExtraField::read_from(&mut reader)));
        }
        if this.flags.contains(F_NAME) {
            this.filename = Some(try!(read_cstring(&mut reader)));
        }
        if this.flags.contains(F_COMMENT) {
            this.comment = Some(try!(read_cstring(&mut reader)));
        }
        if this.flags.contains(F_HCRC) {
            this.header_crc = Some(try!(reader.read_u16::<LittleEndian>()));
        }
        Ok(this)
    }
}

fn read_cstring<R>(mut reader: R) -> io::Result<CString>
    where R: io::Read
{
    let mut buf = Vec::new();
    loop {
        let b = try!(reader.read_u8());
        if b == 0 {
            return Ok(unsafe { CString::from_vec_unchecked(buf) });
        }
        buf.push(b);
    }
}
fn invalid_data_error<T>(description: &str) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, description))
}

#[derive(Debug, Clone)]
pub struct ExtraField {
    pub id: [u8; 2],
    pub data: Vec<u8>,
}
impl ExtraField {
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        let mut id = [0; 2];
        try!(reader.read_exact(&mut id));

        let data_size = try!(reader.read_u16::<LittleEndian>()) as usize;
        let mut data = vec![0; data_size];
        try!(reader.read_exact(&mut data));

        Ok(ExtraField {
            id: id,
            data: data,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Os {
    Fat,
    Amiga,
    Vms,
    Unix,
    VmCms,
    AtariTos,
    Hpfs,
    Macintosh,
    ZSystem,
    CpM,
    Tops20,
    Ntfs,
    Qdos,
    AcornRiscos,
    Unknown,
    Undefined(u8),
}
impl Os {
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        Ok(match try!(reader.read_u8()) {
            OS_FAT => Os::Fat,
            OS_AMIGA => Os::Amiga,
            OS_VMS => Os::Vms,
            OS_UNIX => Os::Unix,
            OS_VM_CMS => Os::VmCms,
            OS_ATARI_TOS => Os::AtariTos,
            OS_HPFS => Os::Hpfs,
            OS_MACINTOSH => Os::Macintosh,
            OS_Z_SYSTEM => Os::ZSystem,
            OS_CPM => Os::CpM,
            OS_TOPS20 => Os::Tops20,
            OS_NTFS => Os::Ntfs,
            OS_QDOS => Os::Qdos,
            OS_ACORN_RISCOS => Os::AcornRiscos,
            OS_UNKNOWN => Os::Unknown,
            os => Os::Undefined(os),
        })
    }
}

pub struct Decoder<R> {
    header: Option<Header>,
    trailer: Option<Trailer>,
    reader: deflate::Decoder<R>,
}
impl<R> Decoder<R>
    where R: io::Read
{
    pub fn new(reader: R) -> Self {
        Decoder {
            header: None,
            trailer: None,
            reader: deflate::Decoder::new(reader),
        }
    }
    pub fn header(&mut self) -> io::Result<&Header> {
        if let Some(ref header) = self.header {
            Ok(header)
        } else {
            let header = try!(Header::read_from(self.reader.as_inner_mut()));
            self.header = Some(header);
            self.header()
        }
    }
    pub fn finish(mut self) -> io::Result<(R, Vec<u8>, Trailer)> {
        let mut buf = Vec::new();
        try!(self.read_to_end(&mut buf));
        Ok((self.reader.into_reader(), buf, self.trailer.unwrap()))
    }
}
impl<R> io::Read for Decoder<R>
    where R: io::Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.header.is_none() {
            try!(self.header());
        }
        if self.trailer.is_some() {
            return Ok(0);
        }
        let read_size = try!(self.reader.read(buf));
        if read_size == 0 {
            let trailer = try!(Trailer::read_from(self.reader.as_inner_mut()));
            self.trailer = Some(trailer);
        }
        Ok(read_size)
    }
}
