use std::cmp;
use std::io::{self, Read};

use bit;

#[derive(Debug)]
pub struct TransactionalBitReader<R> {
    pub inner: bit::BitReader<BufferReader<R>>, // TODO: private
    savepoint: bit::BitReaderState,
}
impl<R: Read> TransactionalBitReader<R> {
    pub fn transaction<F, T>(&mut self, f: F) -> io::Result<T>
    where
        F: FnOnce(&mut bit::BitReader<BufferReader<R>>) -> io::Result<T>,
    {
        self.start_transaction();
        let result = f(&mut self.inner);
        if result.is_ok() {
            self.commit_transaction();
        } else {
            self.abort_transaction();
        }
        result
    }
    pub fn new(inner: R) -> Self {
        let inner = bit::BitReader::new(BufferReader::new(inner));
        let savepoint = inner.state();
        TransactionalBitReader { inner, savepoint }
    }
    pub fn start_transaction(&mut self) {
        self.inner.as_inner_mut().start_transaction();
        self.savepoint = self.inner.state();
    }
    pub fn abort_transaction(&mut self) {
        self.inner.as_inner_mut().abort_transaction();
        self.inner.restore(self.savepoint);
    }
    pub fn commit_transaction(&mut self) {
        self.inner.as_inner_mut().commit_transaction();
    }
    pub fn read_bit(&mut self) -> io::Result<bool> {
        match self.inner.read_bit() {
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    self.abort_transaction();
                }
                Err(e)
            }
            Ok(v) => Ok(v),
        }
    }
    pub fn read_bits(&mut self, width: u8) -> io::Result<u16> {
        match self.inner.read_bits(width) {
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    self.abort_transaction();
                }
                Err(e)
            }
            Ok(v) => Ok(v),
        }
    }
}

#[derive(Debug)]
pub struct BufferReader<R> {
    inner: R,
    buf: Vec<u8>,
    in_transaction: bool,
    offset: usize,
}
impl<R> BufferReader<R> {
    pub fn new(inner: R) -> Self {
        BufferReader {
            inner,
            buf: Vec::new(),
            in_transaction: false,
            offset: 0,
        }
    }
    pub fn start_transaction(&mut self) {
        assert!(!self.in_transaction);
        self.in_transaction = true;
        self.offset = 0;
        self.buf.clear();
    }
    pub fn commit_transaction(&mut self) {
        self.in_transaction = false;
        self.offset = 0;
        self.buf.clear();
    }
    pub fn abort_transaction(&mut self) {
        self.offset = 0;
    }
}
impl<R: Read> Read for BufferReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.offset < self.buf.len() {
            let unread_buf_size = self.buf.len() - self.offset;
            let size = cmp::min(buf.len(), unread_buf_size);
            (&mut buf[0..size]).copy_from_slice(&self.buf[self.offset..self.offset + size]);
            self.offset += size;
            return Ok(size);
        }

        let size = self.inner.read(buf)?;
        if self.in_transaction {
            self.buf.extend(&buf[0..size]);
            self.offset += size;
        }
        Ok(size)
    }
}
