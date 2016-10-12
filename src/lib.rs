extern crate byteorder;

use std::io;

macro_rules! invalid_data_error {
    ($fmt:expr) => { invalid_data_error!("{}", $fmt) };
    ($fmt:expr, $($arg:tt)*) => {
        ::std::io::Error::new(::std::io::ErrorKind::InvalidData, format!($fmt, $($arg)*))
    }
}

macro_rules! finish_try {
    ($e:expr) => {
        match $e.unwrap() {
            (inner, None) => inner,
            (inner, error) => return ::Finish::new(inner, error)
        }
    }
}

pub mod gzip;
pub mod zlib;
pub mod deflate;

mod bit;
mod lz77;
mod huffman;
mod checksum;

#[derive(Debug)]
pub struct Finish<T> {
    inner: T,
    error: Option<io::Error>,
}
impl<T> Finish<T> {
    pub fn new(inner: T, error: Option<io::Error>) -> Self {
        Finish {
            inner: inner,
            error: error,
        }
    }
    pub fn unwrap(self) -> (T, Option<io::Error>) {
        (self.inner, self.error)
    }
    pub fn result(self) -> io::Result<T> {
        if let Some(e) = self.error {
            Err(e)
        } else {
            Ok(self.inner)
        }
    }
    pub fn inner(&self) -> &T {
        &self.inner
    }
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
    pub fn error(&self) -> Option<&io::Error> {
        self.error.as_ref()
    }
}
