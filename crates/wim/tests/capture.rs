#![cfg(unix)]
use std::{path::PathBuf, sync::Arc};
use wim::engine::capture::{CaptureConfig, CapturedSource, scan_source};
use wim_format::ParseError;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "native-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn scan_retains_shared_hardlink_streams_and_reads_later_source_contents() {
    let fixture = Fixture::new();
    let file = fixture.0.join("file");
    std::fs::write(&file, b"before").unwrap();
    std::fs::hard_link(&file, fixture.0.join("alias")).unwrap();
    let config = CaptureConfig::default();
    let mut events = Vec::new();
    let plan = scan_source(&fixture.0, true, 4, &config, &mut |event| {
        events.push((
            event.message,
            event.directories,
            event.nondirectories,
            event.bytes,
        ));
        Ok(())
    })
    .unwrap();
    assert_eq!(plan.bindings.len(), 2);
    assert!(Arc::ptr_eq(
        &plan.bindings[0].stream,
        &plan.bindings[1].stream
    ));
    assert!(matches!(
        plan.bindings[0].stream.source,
        CapturedSource::File(_)
    ));
    assert_eq!(events.last(), Some(&(11, 1, 2, 6)));
    assert!(plan.tree.nodes.iter().all(|node| node.main_hash == [0; 20]));
    std::fs::write(&file, b"after!").unwrap();
    let mut bytes = [0; 6];
    plan.bindings[0]
        .stream
        .open()
        .unwrap()
        .read_range(0, &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"after!");
}
#[test]
fn deferred_reader_reports_original_truncation_and_missing_source_errors() {
    let fixture = Fixture::new();
    let file = fixture.0.join("file");
    std::fs::write(&file, b"initial").unwrap();
    let config = CaptureConfig::default();
    let plan = scan_source(&fixture.0, true, 0, &config, &mut |_| Ok(())).unwrap();
    std::fs::write(&file, b"short").unwrap();
    assert_eq!(
        plan.bindings[0]
            .stream
            .open()
            .unwrap()
            .read_range(0, &mut [0; 7]),
        Err(ParseError::ConcurrentModificationDetected)
    );
    std::fs::remove_file(&file).unwrap();
    assert!(matches!(
        plan.bindings[0].stream.open(),
        Err(ParseError::Open)
    ));
}
#[test]
fn configuration_excludes_subtrees_and_keeps_exception_ancestors() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.0.join("exclude")).unwrap();
    std::fs::write(fixture.0.join("exclude/drop"), b"drop").unwrap();
    std::fs::write(fixture.0.join("exclude/keep"), b"keep").unwrap();
    let config = CaptureConfig::parse_text(
        b"[ExclusionList]\n/exclude\n[ExclusionException]\n/exclude/keep\n",
    )
    .unwrap();
    let plan = scan_source(&fixture.0, true, 0, &config, &mut |_| Ok(())).unwrap();
    assert_eq!(plan.tree.nodes.len(), 3);
    assert_eq!(plan.bindings.len(), 1);
    assert_eq!(
        CaptureConfig::parse_text(b"[ExclusionList]\nrelative/path\n").unwrap_err(),
        ParseError::InvalidCaptureConfig
    );
}
#[test]
fn root_symlinks_are_dereferenced_and_absolute_child_links_are_fixed() {
    let fixture = Fixture::new();
    let directory = fixture.0.join("source");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("file"), b"data").unwrap();
    std::os::unix::fs::symlink(directory.join("file"), directory.join("link")).unwrap();
    let root = fixture.0.join("root");
    std::os::unix::fs::symlink(&directory, &root).unwrap();
    let config = CaptureConfig::default();
    let mut statuses = Vec::new();
    let plan = scan_source(&root, true, 0x184, &config, &mut |event| {
        statuses.push(event.status);
        Ok(())
    })
    .unwrap();
    assert!(statuses.contains(&3));
    assert_eq!(plan.tree.nodes[0].attributes, 0x10);
    let binding = plan
        .bindings
        .iter()
        .find(|b| matches!(b.stream.source, CapturedSource::Inline(_)))
        .unwrap();
    assert_eq!(plan.tree.nodes[binding.node].inode_union, 0xa000000c);
    let CapturedSource::Inline(bytes) = &binding.stream.source else {
        unreachable!()
    };
    assert_eq!(
        u16::from_le_bytes([bytes[4], bytes[5]]),
        u16::from_le_bytes([bytes[2], bytes[3]]) + 2
    );
}

#[test]
fn grafting_at_image_root_clears_long_and_dos_names() {
    let fixture = Fixture::new();
    let mut branch = scan_source(&fixture.0, true, 0, &CaptureConfig::default(), &mut |_| {
        Ok(())
    })
    .unwrap();
    branch.tree.nodes[0].name = b"s\0o\0u\0r\0c\0e\0".to_vec();
    branch.tree.nodes[0].short_name = "SOURCE~1"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut image = wim::engine::capture::CapturePlan::default();
    image.overlay(branch, &[], 0, &mut |_| Ok(())).unwrap();
    assert!(image.tree.nodes[0].name.is_empty());
    assert!(image.tree.nodes[0].short_name.is_empty());
}

#[test]
fn overlay_remaps_and_deduplicates_branch_security_descriptors() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("child"), b"data").unwrap();
    let mut image = scan_source(&fixture.0, true, 0, &CaptureConfig::default(), &mut |_| {
        Ok(())
    })
    .unwrap();
    image.tree.security_descriptors = vec![vec![1], vec![2]];
    let mut branch = image.clone();
    branch.tree.security_descriptors = vec![vec![2], vec![3]];
    branch.tree.nodes[0].security_id = 0;
    branch.tree.nodes[1].security_id = 1;
    image
        .overlay(branch, &["other".encode_utf16().collect()], 0, &mut |_| {
            Ok(())
        })
        .unwrap();
    assert_eq!(
        image.tree.security_descriptors,
        vec![vec![1], vec![2], vec![3]]
    );
    assert_eq!(image.tree.nodes[2].security_id, 1);
    assert_eq!(image.tree.nodes[3].security_id, 2);
}

#[test]
fn mapped_branch_root_clears_dos_name_and_preserves_child_dos_name() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("child"), b"data").unwrap();
    let mut branch = scan_source(&fixture.0, true, 0, &CaptureConfig::default(), &mut |_| {
        Ok(())
    })
    .unwrap();
    branch.tree.nodes[0].short_name = b"R\0O\0O\0T\0".to_vec();
    branch.tree.nodes[1].short_name = b"C\0H\0I\0L\0D\0".to_vec();
    let mut image = wim::engine::capture::CapturePlan::default();
    image
        .overlay(branch, &["target".encode_utf16().collect()], 0, &mut |_| {
            Ok(())
        })
        .unwrap();
    assert!(image.tree.nodes[1].short_name.is_empty());
    assert_eq!(image.tree.nodes[2].short_name, b"C\0H\0I\0L\0D\0");
}
