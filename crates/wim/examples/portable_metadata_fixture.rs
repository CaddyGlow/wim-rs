//! Produce a small immutable-source archive for independent Windows metadata apply gates.
#[cfg(feature = "disk-capture")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use partmgr::partition::PartitionTable;
    use std::{io, path::PathBuf, sync::Arc};
    use virtdisk::Qcow2;
    use wim::{Compression, VolumeCaptureOptions, Wim};
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            io::Error::other("usage: portable_metadata_fixture QCOW2 BACKING OUTPUT_WIM").into(),
        );
    }
    let output = std::path::Path::new(&args[3]);
    if output.exists() {
        return Err(io::Error::other("output already exists").into());
    }
    let disk = Qcow2::open_chain(&args[1], &[PathBuf::from(&args[2])])?;
    let table = PartitionTable::read(Arc::new(disk), 512)?;
    let volume = disk_capture::Volume::open(Arc::new(table.select(3)?))?;
    let selected = [
        vec!["NativeDiskCapture"],
        vec!["Windows", "SysWOW64", "Windows.StateRepositoryCore.dll"],
    ];
    let manifest = volume.capture_manifest_with_filter(|path| {
        selected.iter().any(|wanted| {
            path.iter()
                .zip(wanted)
                .all(|(actual, expected)| actual.iter().copied().eq(expected.encode_utf16()))
                && (path.len() <= wanted.len() || wanted[0] == "NativeDiskCapture")
        })
    })?;
    if !manifest
        .nodes
        .iter()
        .any(|node| node.storage_class == Some(2))
    {
        return Err(
            io::Error::other("source selection contains no class-2 storage fixture").into(),
        );
    }
    let mut archive = Wim::new(Compression::Lzx)?;
    let (index, audit) = archive.capture_ntfs_with_audit(
        manifest,
        "portable metadata fixture",
        &VolumeCaptureOptions {
            volume_aliases: vec![r"\??\C:".into()],
            ..Default::default()
        },
    )?;
    archive.write(output)?;
    archive.verify()?;
    println!("image={} audit={audit:?}", index.get());
    Ok(())
}
#[cfg(not(feature = "disk-capture"))]
fn main() {
    eprintln!("requires --features disk-capture");
    std::process::exit(1);
}
