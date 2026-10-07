extern crate wim_format;
use wim_format::metadata::{Metadata, StreamType};
fn main() {
    let b = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let m = match Metadata::parse(&b) {
        Ok(m) => m,
        Err(e) => {
            println!("status={}", e as u32);
            return;
        }
    };
    if m.nodes.is_empty() {
        println!("status=49");
        return;
    }
    let mut pending = if m.nodes.is_empty() {
        vec![]
    } else {
        vec![(0, "/".to_string())]
    };
    while let Some((i, path)) = pending.pop() {
        let e = m.inode_entry(i).unwrap();
        let links = m
            .nodes
            .iter()
            .filter(|n| n.inode == m.nodes[i].inode)
            .count();
        let named = e
            .streams
            .iter()
            .filter(|s| s.kind == StreamType::Data && !s.name.is_empty())
            .count();
        println!(
            "{} attrs={} links={} streams={}",
            path, e.attributes, links, named
        );
        for &c in m.nodes[i].children.iter().rev() {
            let n = &m.nodes[c].entry.name;
            let name = String::from_utf16_lossy(
                &n.chunks_exact(2)
                    .map(|u| u16::from_le_bytes([u[0], u[1]]))
                    .collect::<Vec<_>>(),
            );
            pending.push((
                c,
                format!("{}{}{}", path, if path == "/" { "" } else { "/" }, name),
            ));
        }
    }
    println!("status=0");
}
