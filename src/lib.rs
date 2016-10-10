#[macro_use]
extern crate bitflags;
extern crate byteorder;

macro_rules! invalid_data_error {
    ($fmt:expr) => { invalid_data_error!("{}", $fmt) };
    ($fmt:expr, $($arg:tt)*) => {
        ::std::io::Error::new(::std::io::ErrorKind::InvalidData, format!($fmt, $($arg)*))
    }
}

pub mod lz77;

pub mod deflate;
pub mod gzip;
pub mod zlib;

mod bit;
mod huffman;
mod checksum;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
