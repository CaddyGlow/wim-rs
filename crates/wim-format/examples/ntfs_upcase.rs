use std::io::{self, Write};
fn main() -> io::Result<()> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for unit in 0..=u16::MAX {
        output.write_all(&wim_format::ntfs_upcase::uppercase(unit).to_le_bytes())?;
    }
    output.flush()
}
