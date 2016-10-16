pub use self::decode::Decoder;
pub use self::encode::Encoder;
pub use self::encode::EncodeOptions;
pub use self::encode::DEFAULT_BLOCK_SIZE;

mod decode;
mod encode;
mod symbol;

#[derive(Debug, Clone, Copy)]
enum BlockType {
    Raw = 0b00,
    Fixed = 0b01,
    Dynamic = 0b10,
}
