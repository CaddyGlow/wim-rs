//! Native resource extraction by hash for interoperability tests.
use std::{env, fs, process::ExitCode};
use wim_format::archive::Archive;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: read_blob WIM SHA1 OUTPUT");
        return ExitCode::FAILURE;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut hash = [0; 20];
        if args[2].len() != 40 || !args[2].is_ascii() {
            return Err("invalid SHA1".into());
        }
        for (i, byte) in hash.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&args[2][i * 2..i * 2 + 2], 16)?;
        }
        let file = fs::read(&args[1])?;
        let archive = Archive::open(&file).map_err(|e| format!("open error {}", e.as_i32()))?;
        let bytes = archive
            .read_blob(&hash)
            .map_err(|e| format!("read error {}", e.as_i32()))?;
        fs::write(&args[3], bytes)?;
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
