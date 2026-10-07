//! Differential probe for the fixed-header slice, not an implementation of open_wim.

use std::{env, fs, process::ExitCode};
use wim_format::{Header, ParseError};
fn probe(path: &str) -> Result<u32, ParseError> {
    let bytes = fs::read(path).map_err(|_| ParseError::Read)?;
    let h = Header::parse_seekable(&bytes)?;
    h.validate_compression()?;
    Ok(if h.boot_index > h.image_count {
        0
    } else {
        h.boot_index
    })
}
fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: header_status PATH");
        return ExitCode::FAILURE;
    };
    match probe(&path) {
        Ok(boot) => println!("0 {boot}"),
        Err(e) => println!("{}", e.as_i32()),
    }
    ExitCode::SUCCESS
}
