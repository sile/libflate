/// https://tools.ietf.org/html/rfc1952
use std::io;
use std::io::Read;
use std::io::Write;
use std::ffi::CString;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
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
    pub fn write_to<W>(&self, mut writer: W) -> io::Result<()>
        where W: io::Write
    {
        let byte = match *self {
            CompressionMethod::Deflate => COMPRESSION_METHOD_DEFLATE,
            CompressionMethod::Undefined(b) => b,
        };
        writer.write_u8(byte)
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
    pub fn write_to<W>(&self, mut writer: W) -> io::Result<()>
        where W: io::Write
    {
        try!(writer.write_all(&self.id));
        try!(self.compression_method.write_to(&mut writer));
        try!(writer.write_u8(self.flags.bits())); // TODO: gurantees consistency
        try!(writer.write_u32::<LittleEndian>(self.modification_time));
        try!(writer.write_u8(self.extra_flags));
        try!(self.os.write_to(&mut writer));
        if let Some(ref x) = self.extra_field {
            try!(x.write_to(&mut writer));
        }
        if let Some(ref x) = self.filename {
            try!(writer.write_all(x.as_bytes_with_nul()));
        }
        if let Some(ref x) = self.comment {
            try!(writer.write_all(x.as_bytes_with_nul()));
        }
        if let Some(x) = self.header_crc {
            try!(writer.write_u16::<LittleEndian>(x));
        }
        Ok(())
    }
    pub fn read_from<R>(mut reader: R) -> io::Result<Self>
        where R: io::Read
    {
        let mut this = Header::default();
        try!(reader.read_exact(&mut this.id));
        if this.id != GZIP_ID {
            return Err(invalid_data_error!("Unexpected GZIP ID: value={:?}, \
                                                    expected={:?}",
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
    pub fn write_to<W>(&self, mut writer: W) -> io::Result<()>
        where W: io::Write
    {
        try!(writer.write_all(&self.id));
        try!(writer.write_u16::<LittleEndian>(self.data.len() as u16()));
        try!(writer.write_all(&self.data));
        Ok(())
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
    pub fn as_byte(&self) -> u8 {
        match *self {
            Os::Fat => OS_FAT,
            Os::Amiga => OS_AMIGA,
            Os::Vms => OS_VMS,
            Os::Unix => OS_UNIX,
            Os::VmCms => OS_VM_CMS,
            Os::AtariTos => OS_ATARI_TOS,
            Os::Hpfs => OS_HPFS,
            Os::Macintosh => OS_MACINTOSH,
            Os::ZSystem => OS_Z_SYSTEM,
            Os::CpM => OS_CPM,
            Os::Tops20 => OS_TOPS20,
            Os::Ntfs => OS_NTFS,
            Os::Qdos => OS_QDOS,
            Os::AcornRiscos => OS_ACORN_RISCOS,
            Os::Unknown => OS_UNKNOWN,
            Os::Undefined(os) => os,
        }
    }
    pub fn write_to<W>(&self, mut writer: W) -> io::Result<()>
        where W: io::Write
    {
        writer.write_u8(self.as_byte())
    }
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

pub struct EncodeOptions;

enum EncodePhase {
    Header,
    Data,
}

pub struct Encoder<W>
    where W: io::Write
{
    phase: EncodePhase,
    header: Header,
    writer: W,
}
impl<W> Encoder<W>
    where W: io::Write
{
    pub fn new(writer: W) -> Self {
        Encoder {
            phase: EncodePhase::Header,
            header: Header::default(),
            writer: writer,
        }
    }
}
impl<W> io::Write for Encoder<W>
    where W: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.phase {
            EncodePhase::Header => {
                try!(self.header.write_to(&mut self.writer));
                self.phase = EncodePhase::Data;
                self.write(buf)
            }
            EncodePhase::Data => panic!(),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!()
    }
}
impl<W> Drop for Encoder<W>
    where W: io::Write
{
    fn drop(&mut self) {
        self.flush().unwrap();
    }
}

pub struct Decoder<R> {
    header: Header,
    trailer: Option<Trailer>,
    reader: deflate::Decoder<R>,
    read_size: u32,
}
impl<R> Decoder<R>
    where R: io::Read
{
    pub fn new(mut reader: R) -> io::Result<Self> {
        let header = try!(Header::read_from(&mut reader));
        Ok(Decoder {
            header: header,
            trailer: None,
            reader: deflate::Decoder::new(reader),
            read_size: 0,
        })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn finish(mut self) -> io::Result<(R, Vec<u8>, Trailer)> {
        let mut buf = Vec::new();
        try!(self.read_to_end(&mut buf));
        Ok((self.reader.into_inner(), buf, self.trailer.unwrap()))
    }
}
impl<R> io::Read for Decoder<R>
    where R: io::Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.trailer.is_some() {
            return Ok(0);
        }
        let read_size = try!(self.reader.read(buf));
        self.read_size = self.read_size.wrapping_add(read_size as u32);
        if read_size == 0 {
            let trailer = try!(Trailer::read_from(self.reader.as_inner_mut()));
            if trailer.input_size != self.read_size {
                Err(invalid_data_error!("Input size mismatched: value={}, expected={}",
                                        self.read_size,
                                        trailer.input_size))
            } else {
                self.trailer = Some(trailer);
                self.read(buf)
            }
        } else {
            Ok(read_size)
        }
    }
}
