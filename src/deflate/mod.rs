pub use self::decode::Decoder;
pub use self::encode::Encoder;
pub use self::encode::EncodeOptions;

mod decode;
mod encode;
mod symbol;

#[derive(Debug, Clone, Copy)]
enum BlockType {
    Raw = 0b00,
    Fixed = 0b01,
    Dynamic = 0b10,
}
