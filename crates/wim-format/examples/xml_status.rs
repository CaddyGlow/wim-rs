//! Differential probe for XML parsing and public property operations.
use std::{env, fs};
use wim_format::xml::XmlInfo;
fn main() {
    let args: Vec<_> = env::args().collect();
    let bytes = fs::read(&args[1]).unwrap();
    let mut x = match XmlInfo::parse_utf16le(&bytes) {
        Ok(x) => x,
        Err(e) => {
            println!("{}", e as i32);
            return;
        }
    };
    if args.len() > 2 {
        let image = args[2].parse().unwrap();
        let value = if args[4] == "@none" {
            None
        } else {
            Some(args[4].as_str())
        };
        let code = x
            .set_property(image, &args[3], value)
            .map_or_else(|e| e as i32, |()| 0);
        println!("{code}");
        let v = x.get_property(image, &args[3]);
        println!(
            "{}",
            v.map(|v| v
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>())
                .unwrap_or_else(|| "@none".into())
        );
    } else {
        println!("0");
    }
    print!("{}", x.to_xml().unwrap());
}
