#![cfg(not(windows))]
use std::{ffi::CString, ptr};
use wim::ffi::*;
use wim_format::{
    Compression,
    image_build::{ImageBuilder, NewImage},
    metadata_write::{OwnedDentry, OwnedMetadata},
    repack::WriteOptions,
};

#[test]
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn update_command_layout_matches_original_linux_x64_probe() {
    assert_eq!(std::mem::size_of::<UpdateCommand>(), 40);
    assert_eq!(std::mem::offset_of!(UpdateCommand, data), 8);
    assert_eq!(std::mem::size_of::<AddCommand>(), 32);
    assert_eq!(std::mem::size_of::<DeleteCommand>(), 16);
    assert_eq!(std::mem::size_of::<RenameCommand>(), 24);
    assert_eq!(std::mem::size_of::<UpdateProgress>(), 24);
}

#[test]
fn failed_batch_restores_empty_image_after_captured_tree_attachment() {
    let directory = std::env::temp_dir().join(format!("wim-update-capture-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("deferred");
    std::fs::write(&file, b"original stream").unwrap();
    let source = CString::new(directory.to_str().unwrap()).unwrap();
    let mut handle = ptr::null_mut();
    let commands = [
        UpdateCommand {
            op: 0,
            data: UpdateCommandData {
                add: AddCommand {
                    fs_source_path: source.as_ptr().cast_mut(),
                    wim_target_path: c"/".as_ptr().cast_mut(),
                    config_file: ptr::null_mut(),
                    add_flags: 0,
                },
            },
        },
        UpdateCommand {
            op: 1,
            data: UpdateCommandData {
                delete: DeleteCommand {
                    wim_path: c"missing".as_ptr().cast_mut(),
                    delete_flags: 0,
                },
            },
        },
    ];
    // SAFETY: Handle, command array and terminated paths stay live through each call.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_add_empty_image(handle, c"Capture".as_ptr(), ptr::null_mut()),
            0
        );
        assert_eq!(wimlib_update_image(handle, 1, commands.as_ptr(), 2, 0), 49);
        assert!(matches!((&(*handle).images)[0], HandleImage::Empty(_)));
        assert!((*handle).dirty_images.is_empty());
        assert!((*handle).owned_blobs.is_empty());
        assert_eq!((*handle).header.flags & 0x80, 0);
        assert_eq!(wimlib_update_image(handle, 1, commands.as_ptr(), 1, 0), 0);
        assert_eq!((*handle).header.flags & 0x80, 0x80);
        let HandleImage::Owned(image) = &(&(*handle).images)[0] else {
            panic!("captured tree was not retained");
        };
        let pending = image.pending.as_ref().unwrap().lock().unwrap();
        let plan = pending.capture.as_ref().unwrap();
        assert_eq!(plan.bindings.len(), 1);
        assert_eq!(plan.bindings[0].stream.size, 15);
        assert!(plan.tree.nodes.iter().all(|node| node.main_hash == [0; 20]));
        drop(pending);
        wimlib_free(handle);
    }
    assert_eq!(std::fs::read(&file).unwrap(), b"original stream");
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
unsafe extern "C" fn cancel(
    _event: i32,
    _info: *mut ProgressInfo,
    context: *mut std::ffi::c_void,
) -> i32 {
    // SAFETY: Test supplies live call/abort counters for every callback.
    let counters = unsafe { &mut *context.cast::<(usize, usize)>() };
    counters.0 += 1;
    i32::from(counters.0 == counters.1)
}
#[test]
fn cancellation_at_each_command_boundary_restores_metadata_and_blob_counts() {
    let path = std::env::temp_dir().join(format!("wim-update-rollback-{}.wim", std::process::id()));
    let mut builder = ImageBuilder::new([7; 16]);
    let hash = builder.add_blob(b"shared stream").unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 2];
    let mut file = OwnedDentry::new(
        "file".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        0x80,
    );
    file.main_hash = hash;
    file.inode_union = 1;
    let mut alias = file.clone();
    alias.name = "alias".encode_utf16().flat_map(u16::to_le_bytes).collect();
    builder
        .add_image(NewImage {
            metadata: OwnedMetadata {
                security_descriptors: Vec::new(),
                nodes: vec![root, file, alias],
            },
            name: Some("Rollback".into()),
            description: None,
            properties: Vec::new(),
        })
        .unwrap();
    std::fs::write(
        &path,
        builder
            .write(WriteOptions {
                compression: Compression::None,
                chunk_size: 0,
                integrity: false,
            })
            .unwrap(),
    )
    .unwrap();
    let name = CString::new(path.to_str().unwrap()).unwrap();
    for boundary in 1usize..=4 {
        let mut handle = ptr::null_mut();
        // SAFETY: Handles, command array, terminated paths and counters are live;
        // each handle is accessed sequentially and freed exactly once.
        unsafe {
            assert_eq!(wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
            let before = (*handle)
                .lookup
                .as_ref()
                .unwrap()
                .find(&hash)
                .unwrap()
                .reference_count;
            let commands = [
                UpdateCommand {
                    op: 1,
                    data: UpdateCommandData {
                        delete: DeleteCommand {
                            wim_path: c"file".as_ptr().cast_mut(),
                            delete_flags: 0,
                        },
                    },
                },
                UpdateCommand {
                    op: 1,
                    data: UpdateCommandData {
                        delete: DeleteCommand {
                            wim_path: c"alias".as_ptr().cast_mut(),
                            delete_flags: 0,
                        },
                    },
                },
            ];
            let mut counters = (0, boundary);
            wimlib_register_progress_function(
                handle,
                Some(cancel),
                (&mut counters as *mut (usize, usize)).cast(),
            );
            assert_eq!(
                wimlib_update_image(handle, 1, commands.as_ptr(), commands.len(), 1),
                76
            );
            assert_eq!(counters.0, boundary);
            assert_eq!(
                (*handle)
                    .lookup
                    .as_ref()
                    .unwrap()
                    .find(&hash)
                    .unwrap()
                    .reference_count,
                before
            );
            assert!(matches!((&(*handle).images)[0], HandleImage::Source(1)));
            assert!((*handle).dirty_images.is_empty());
            wimlib_register_progress_function(handle, None, ptr::null_mut());
            assert_eq!(wimlib_verify_wim(handle, 0), 0);
            wimlib_free(handle);
        }
    }
    std::fs::remove_file(path).unwrap();
}

use wim::engine::handles::HandleImage;
