libflate
========

[![libflate](http://meritbadge.herokuapp.com/libflate)](https://crates.io/crates/libflate)
[![Build Status](https://travis-ci.org/sile/libflate.svg?branch=master)](https://travis-ci.org/sile/libflate)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A Rust implementation of DEFLATE algorithm and related formats (ZLIB, GZIP).

Documentation
-------------

See [RustDoc Documentation](https://docs.rs/libflate).

The documentation includes some examples.

Installation
------------

Add following lines to your `Cargo.toml`:

```toml
[dependencies]
libflate = "0.1"
```

An Example
----------

Below is a command to decode GZIP stream that is read from the standard input:

```rust
extern crate libflate;

use std::io;
use libflate::gzip::Decoder;

fn main() {
    let mut input = io::stdin();
    let mut decoder = Decoder::new(&mut input).unwrap();
    io::copy(&mut decoder, &mut io::stdout()).unwrap();
}
```

An Informal Benchmark
---------------------

A brief comparison with [flate2](https://github.com/alexcrichton/flate2-rs) and
[inflate](https://github.com/PistonDevelopers/inflate):

```bash
$ cd libflate/flate_bench/
$ curl -O https://dumps.wikimedia.org/enwiki/latest/enwiki-latest-all-titles-in-ns0.gz
$ gzip -d enwiki-latest-all-titles-in-ns0.gz
$ ls -lh enwiki-latest-all-titles-in-ns0
-rw-rw-rw- 1 foo foo 257M 10月  3 17:22 enwiki-latest-all-titles-in-ns0

$ cargo run --release -- enwiki-latest-all-titles-in-ns0
# ENCODE (input_size=268799390)
- libflate: elapsed= 6.487679s, size=93326863
-   flate2: elapsed=10.715379s, size=72043928

# DECODE (input_size=72043928)
- libflate: elapsed=1.711679s, size=268799390
-   flate2: elapsed=0.975283s, size=268799390
-  inflate: elapsed=1.918320s, size=268799390
```

References
----------

- DEFLATE: [RFC-1951](https://tools.ietf.org/html/rfc1951)
- ZLIB: [RFC-1950](https://tools.ietf.org/html/rfc1950)
- GZIP: [RFC-1952](https://tools.ietf.org/html/rfc1952)
