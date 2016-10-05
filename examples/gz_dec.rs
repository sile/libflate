extern crate byteorder;
extern crate libflate;

use std::io;
use std::io::Read;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;

fn main() {
    let mut reader = io::stdin();
    let id1 = reader.read_u8().unwrap();
    let id2 = reader.read_u8().unwrap();
    let mode = reader.read_u8().unwrap();
    let flag = reader.read_u8().unwrap();
    let mtime = reader.read_u32::<LittleEndian>().unwrap();
    let xfl = reader.read_u8().unwrap();
    let os = reader.read_u8().unwrap();
    if flag & 0b0001 != 0 {
        panic!();
    }
    if flag & 0b0010 != 0 {
        panic!();
    }
    if flag & 0b0100 != 0 {
        panic!();
    }
    if flag & 0b1000 != 0 {
        // FNAME
        let mut name = String::new();
        loop {
            let b = reader.read_u8().unwrap();
            if b == 0 {
                break;
            }
            name.push(b as char);
        }
        println!("NAME: {}\n", name);
    }
    if flag & 0b10000 != 0 {
        panic!();
    }

    println!("
# HEADER
- id1: {}
- id2: {}
- mode: {}
- flag: {}
- mtime: {}
- xfl: {}
- os: {}
",
             id1,
             id2,
             mode,
             flag,
             mtime,
             xfl,
             os);

    let mut dec = libflate::deflate::Decoder::new(reader);
    let mut buf = Vec::new();
    dec.read_to_end(&mut buf).unwrap();
    println!("
# BODY
{}
",
             String::from_utf8_lossy(&buf));
}
