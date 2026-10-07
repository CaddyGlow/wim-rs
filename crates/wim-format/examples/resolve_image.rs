use wim_format::xml::XmlInfo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let xml = XmlInfo::parse(&std::fs::read_to_string(
        args.next().ok_or("XML path required")?,
    )?)?;
    for selector in args {
        println!("{}", xml.resolve_image(Some(&selector)));
    }
    Ok(())
}
