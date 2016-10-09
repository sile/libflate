#[macro_use]
extern crate bitflags;
extern crate byteorder;

pub mod lz77;
pub mod huffman;

pub mod deflate;
pub mod gzip;

// TODO: mod checksum

// TODO: private
pub mod bit;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
