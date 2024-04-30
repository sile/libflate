#[cfg(not(feature = "std"))]
fn main() {}

#[cfg(feature = "std")]
fn main() {
    use clap::Parser;
    use libflate::gzip;
    use libflate::zlib;
    use std::fs;
    use std::io;
    use std::io::Read;
    use std::io::Write;

    #[derive(Parser)]
    struct Args {
        #[clap(short, long, default_value = "-")]
        input: String,

        #[clap(short, long, default_value = "-")]
        output: String,

        #[clap(short, long)]
        verbose: bool,

        #[clap(subcommand)]
        command: Command,
    }

    #[derive(clap::Subcommand)]
    enum Command {
        Copy,
        ByteRead {
            #[clap(short, long, default_value = "1")]
            unit: usize,
        },
        GzipDecode,
        GzipDecodeMulti,
        GzipEncode,
        ZlibDecode,
        ZlibEncode,
    }

    let args = Args::parse();
    let input_filename = &args.input;
    let input: Box<dyn io::Read> = if input_filename == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(
            fs::File::open(input_filename).expect(&format!("Can't open file: {}", input_filename)),
        )
    };
    let mut input = io::BufReader::new(input);

    let output_filename = &args.output;
    let output: Box<dyn io::Write> = if output_filename == "-" {
        Box::new(io::stdout())
    } else if output_filename == "/dev/null" {
        Box::new(io::sink())
    } else {
        Box::new(
            fs::File::create(output_filename)
                .expect(&format!("Can't create file: {}", output_filename)),
        )
    };
    let mut output = io::BufWriter::new(output);

    let verbose = args.verbose;
    match args.command {
        Command::Copy => {
            io::copy(&mut input, &mut output).expect("Coyping failed");
        }
        Command::ByteRead { unit } => {
            let mut buf = vec![0; unit];
            let mut reader = input;
            let mut count = 0;
            while let Ok(size) = reader.read(&mut buf) {
                if size == 0 {
                    break;
                }
                count += size;
            }
            println!("COUNT: {}", count);
        }
        Command::GzipDecode => {
            let mut decoder = gzip::Decoder::new(input).expect("Read GZIP header failed");
            if verbose {
                let _ = writeln!(&mut io::stderr(), "HEADER: {:?}", decoder.header());
            }
            io::copy(&mut decoder, &mut output).expect("Decoding GZIP stream failed");
        }
        Command::GzipDecodeMulti => {
            let mut decoder = gzip::MultiDecoder::new(input).expect("Read GZIP header failed");
            io::copy(&mut decoder, &mut output).expect("Decoding GZIP stream failed");
        }
        Command::GzipEncode => {
            let mut encoder = gzip::Encoder::new(output).unwrap();
            io::copy(&mut input, &mut encoder).expect("Encoding GZIP stream failed");
            encoder.finish().into_result().unwrap();
        }
        Command::ZlibDecode => {
            let mut decoder = zlib::Decoder::new(input).expect("Read ZLIB header failed");
            if verbose {
                let _ = writeln!(&mut io::stderr(), "HEADER: {:?}", decoder.header());
            }
            io::copy(&mut decoder, &mut output).expect("Decoding ZLIB stream failed");
        }
        Command::ZlibEncode => {
            let mut encoder = zlib::Encoder::new(output).unwrap();
            io::copy(&mut input, &mut encoder).expect("Encoding ZLIB stream failed");
            encoder.finish().into_result().unwrap();
        }
    }
}
