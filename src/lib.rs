//! A Rust implementation of DEFLATE algorithm and related formats (ZLIB, GZIP).

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

pub use finish::Finish;
extern crate alloc;

macro_rules! invalid_data_error {
    ($fmt:expr) => {
        ::core2::io::Error::new(::core2::io::ErrorKind::InvalidData, $fmt)
    };
    ($fmt:expr, $($arg:tt)*) => {
        ::core2::io::Error::new(::core2::io::ErrorKind::InvalidData, format!($fmt, $($arg)*))
    };
}

macro_rules! finish_try {
    ($e:expr) => {
        match $e.unwrap() {
            (inner, None) => inner,
            (inner, error) => return crate::finish::Finish::new(inner, error),
        }
    };
}

pub mod deflate;
pub mod finish;
pub mod gzip;
pub mod lz77;
pub mod non_blocking;
pub mod zlib;

mod bit;
mod checksum;
mod huffman;
mod util;
