#![cfg(windows)]

use std::io::{Read, Write};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::{OpenOptionsExt, symlink_file};
use std::{fs, path::PathBuf};
use wim::{CaptureOptions, Compression, ImageIndex, OpenOptions, Wim};

struct Fixture(PathBuf);
fn stream_path(path: &std::path::Path, name: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(":");
    value.push(name);
    PathBuf::from(value)
}
fn unpaired_name() -> std::ffi::OsString {
    let mut units: Vec<u16> = "unpaired-".encode_utf16().collect();
    units.push(0xd800);
    std::ffi::OsString::from_wide(&units)
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires elevated Windows, NTFS, and DISM for independent apply validation"]
fn named_streams_survive_capture_write_and_apply_on_files_directories_and_hard_links() {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "wim-windows-capture-streams-{}",
        std::process::id()
    )));
    fs::create_dir(&fixture.0).unwrap();
    let source = fixture.0.join("source");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("directory")).unwrap();
    fs::write(source.join("file"), b"main content").unwrap();
    fs::write(source.join("file:Zone.Identifier"), b"zone content").unwrap();
    fs::write(source.join("file:empty"), b"").unwrap();
    fs::write(source.join("file:unicode-\u{03bb}"), b"unicode content").unwrap();
    fs::write(source.join(unpaired_name()), b"unpaired filename").unwrap();
    let mut stream = source.join("file").into_os_string();
    stream.push(":");
    stream.push(unpaired_name());
    fs::write(PathBuf::from(stream), b"unpaired stream").unwrap();
    fs::write(source.join("directory:metadata"), b"directory content").unwrap();
    fs::write(stream_path(&source, "root-metadata"), b"root content").unwrap();
    fs::hard_link(source.join("file"), source.join("alias")).unwrap();
    for index in 0u8..160 {
        fs::write(source.join(format!("file:stream-{index}")), [index]).unwrap();
    }
    symlink_file("file", source.join("link")).unwrap();
    let mut link_stream = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(source.join("link:link-only"))
        .unwrap();
    link_stream.write_all(b"link stream").unwrap();
    drop(link_stream);
    let mut link_data = fs::OpenOptions::new()
        .write(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(source.join("link::$DATA"))
        .unwrap();
    link_data.write_all(b"link-owned unnamed data").unwrap();
    drop(link_data);
    assert_eq!(fs::read(source.join("file")).unwrap(), b"main content");
    assert!(fs::metadata(source.join("file:link-only")).is_err());
    assert_eq!(wim::ffi::wimlib_global_init(4 | 8), 0);

    let output = fixture.0.join("capture.wim");
    let mut archive = Wim::new(Compression::Lzx).unwrap();
    archive
        .capture_image_with_options(
            &source,
            CaptureOptions {
                strict_security: true,
            },
        )
        .unwrap();
    archive.write(&output).unwrap();
    let mut archive = Wim::open(&output, OpenOptions::default()).unwrap();
    archive.verify().unwrap();
    let destination = fixture.0.join("destination");
    fs::create_dir(&destination).unwrap();
    archive
        .extract_image(ImageIndex::try_from(1).unwrap(), &destination)
        .unwrap();
    assert_streams(&destination);
    assert_hard_link_streams(&destination);
    let independent = fixture.0.join("dism-destination");
    fs::create_dir(&independent).unwrap();
    let result = std::process::Command::new("dism.exe")
        .arg("/Apply-Image")
        .arg(format!("/ImageFile:{}", output.display()))
        .arg("/Index:1")
        .arg(format!("/ApplyDir:{}", independent.display()))
        .arg("/CheckIntegrity")
        .arg(format!("/LogPath:{}", fixture.0.join("dism.log").display()))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "DISM failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_streams(&independent);
    assert_hard_link_streams(&independent);
}

fn assert_hard_link_streams(destination: &std::path::Path) {
    fs::write(
        destination.join("alias:Zone.Identifier"),
        b"changed through alias",
    )
    .unwrap();
    assert_eq!(
        fs::read(destination.join("file:Zone.Identifier")).unwrap(),
        b"changed through alias"
    );
}

fn assert_streams(destination: &std::path::Path) {
    assert_eq!(
        fs::read(destination.join(unpaired_name())).unwrap(),
        b"unpaired filename"
    );
    let mut stream = destination.join("file").into_os_string();
    stream.push(":");
    stream.push(unpaired_name());
    assert_eq!(fs::read(PathBuf::from(stream)).unwrap(), b"unpaired stream");
    for filename in ["file", "alias"] {
        for index in 0u8..160 {
            assert_eq!(
                fs::read(destination.join(format!("{filename}:stream-{index}"))).unwrap(),
                [index]
            );
        }
        assert_eq!(
            fs::read(destination.join(filename)).unwrap(),
            b"main content"
        );
        assert_eq!(
            fs::read(destination.join(format!("{filename}:Zone.Identifier"))).unwrap(),
            b"zone content"
        );
        assert_eq!(
            fs::read(destination.join(format!("{filename}:unicode-\u{03bb}"))).unwrap(),
            b"unicode content"
        );
        assert_eq!(
            fs::metadata(destination.join(format!("{filename}:empty")))
                .unwrap()
                .len(),
            0
        );
    }
    assert!(
        fs::symlink_metadata(destination.join("link"))
            .unwrap()
            .is_symlink()
    );
    let mut link_stream = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(destination.join("link:link-only"))
        .unwrap();
    let mut bytes = Vec::new();
    link_stream.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"link stream");
    let mut link_data = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(destination.join("link::$DATA"))
        .unwrap();
    bytes.clear();
    link_data.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"link-owned unnamed data");
    assert!(fs::metadata(destination.join("file:link-only")).is_err());
    assert_eq!(
        fs::read(destination.join("directory:metadata")).unwrap(),
        b"directory content"
    );
    assert_eq!(
        fs::read(stream_path(destination, "root-metadata")).unwrap(),
        b"root content"
    );
}

#[test]
#[ignore = "requires elevated Windows 10 1803+, NTFS case-sensitive directories"]
fn capture_rejects_directory_case_sensitivity_that_apply_cannot_restore() {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "wim-windows-capture-case-sensitive-{}",
        std::process::id()
    )));
    fs::create_dir(&fixture.0).unwrap();
    let result = std::process::Command::new("fsutil.exe")
        .args(["file", "setCaseSensitiveInfo"])
        .arg(&fixture.0)
        .arg("enable")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    fs::write(fixture.0.join("file"), b"case policy must survive").unwrap();
    assert_eq!(wim::ffi::wimlib_global_init(4 | 8), 0);
    let mut archive = Wim::new(Compression::Lzx).unwrap();
    assert_eq!(
        archive.capture_image_with_options(
            &fixture.0,
            CaptureOptions {
                strict_security: true
            }
        ),
        Err(wim::Error::Engine(
            wim_format::ParseError::Unsupported as i32
        ))
    );
    assert_eq!(archive.info().unwrap().image_count, 0);
}
