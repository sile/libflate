#[macro_use]
extern crate bitflags;
extern crate byteorder;

pub mod lz77;
pub mod huffman;

pub mod deflate;
pub mod gzip;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
