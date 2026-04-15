//! The interface and implementations of LZ77 compression algorithm.
//!
//! LZ77 is a compression algorithm used in [DEFLATE](https://tools.ietf.org/html/rfc1951).
pub use libflate_lz77::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deflate;
    use crate::deflate::symbol::Symbol;
    use alloc::{vec, vec::Vec};
    use no_std_io2::io::{Read as _, Write as _};

    #[test]
    // See: https://github.com/sile/libflate/issues/21
    fn issue21() {
        let mut enc = DefaultLz77Encoder::new();
        let mut sink = Vec::<Symbol>::new();
        enc.encode(b"aaaaa", &mut sink);
        enc.flush(&mut sink);
        assert_eq!(
            sink,
            vec![
                Symbol::Code(Code::Literal(97)),
                Symbol::Code(Code::Pointer {
                    length: 4,
                    backward_distance: 1
                })
            ]
        );
    }

    #[test]
    fn no_compression_encoder_works_with_deflate() {
        let options = deflate::EncodeOptions::with_lz77(NoCompressionLz77Encoder::new());
        let mut encoder = deflate::Encoder::with_options(Vec::new(), options);
        encoder.write_all(b"hello world").unwrap();
        let encoded = encoder.finish().into_result().unwrap();

        let mut decoder = deflate::Decoder::new(&encoded[..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, b"hello world");
    }

    #[test]
    fn default_encoder_works_with_deflate() {
        let options = deflate::EncodeOptions::with_lz77(DefaultLz77Encoder::new());
        let mut encoder = deflate::Encoder::with_options(Vec::new(), options);
        encoder.write_all(b"hello hello hello").unwrap();
        let encoded = encoder.finish().into_result().unwrap();

        let mut decoder = deflate::Decoder::new(&encoded[..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, b"hello hello hello");
    }
}
