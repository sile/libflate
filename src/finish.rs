use std::io;

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
