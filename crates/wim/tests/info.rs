use wim::ffi::{
    WimInfo, wimlib_create_new_wim, wimlib_free, wimlib_get_wim_info, wimlib_set_wim_info,
};

#[test]
fn wim_info_has_original_layout_and_masked_mutations_are_atomic() {
    assert_eq!(std::mem::size_of::<WimInfo>(), 88);
    assert_eq!(std::mem::offset_of!(WimInfo, total_bytes), 40);
    assert_eq!(std::mem::offset_of!(WimInfo, flags), 48);
    let mut handle = std::ptr::null_mut();
    // SAFETY: Handle is created here, input/output info are valid and uniquely borrowed.
    unsafe {
        assert_eq!(wimlib_create_new_wim(1, &mut handle), 0);
        let mut info = WimInfo::default();
        assert_eq!(wimlib_get_wim_info(handle, &mut info), 0);
        assert_eq!(info.image_count, 0);
        assert_eq!(info.compression_type, 0);
        assert_eq!(info.chunk_size, 0);
        let guid = [0x5a; 16];
        info.guid = guid;
        info.flags = (1 << 3) | (1 << 4);
        assert_eq!(wimlib_set_wim_info(handle, &info, 11), 0);
        let mut updated = WimInfo::default();
        wimlib_get_wim_info(handle, &mut updated);
        assert_eq!(updated.guid, guid);
        assert_eq!(updated.flags & 0x1c, 0x1c);
        info.guid = [0xff; 16];
        info.boot_index = 1;
        assert_eq!(wimlib_set_wim_info(handle, &info, 15), 18);
        wimlib_get_wim_info(handle, &mut updated);
        assert_eq!(updated.guid, guid);
        assert_eq!(wimlib_set_wim_info(handle, &info, 16), 24);
        wimlib_free(handle);
    }
}

#[test]
fn output_compression_settings_repair_chunks_and_preserve_input_info() {
    let mut handle = std::ptr::null_mut();
    // SAFETY: Handle lifetime is owned by this test; all uses are sequential.
    unsafe {
        assert_eq!(wimlib_create_new_wim(1, &mut handle), 0);
        let mut before = WimInfo::default();
        wimlib_get_wim_info(handle, &mut before);
        assert_eq!(wim::ffi::wimlib_set_output_chunk_size(handle, 4096), 0);
        assert_eq!((*handle).output_chunk_size, 4096);
        assert_eq!(wim::ffi::wimlib_set_output_compression_type(handle, 2), 0);
        assert_eq!((*handle).output_chunk_size, 32768);
        assert_eq!(wim::ffi::wimlib_set_output_chunk_size(handle, 17), 15);
        assert_eq!((*handle).output_chunk_size, 32768);
        assert_eq!(wim::ffi::wimlib_set_output_compression_type(handle, 0), 0);
        assert_eq!((*handle).output_chunk_size, 0);
        assert_eq!(wim::ffi::wimlib_set_output_chunk_size(handle, 32768), 15);
        assert_eq!(
            wim::ffi::wimlib_set_output_pack_compression_type(handle, 0),
            16
        );
        assert_eq!((*handle).output_solid_chunk_size, 67108864);
        assert_eq!(
            wim::ffi::wimlib_set_output_pack_compression_type(handle, 1),
            0
        );
        assert_eq!((*handle).output_solid_chunk_size, 32768);
        assert_eq!(
            wim::ffi::wimlib_set_output_pack_chunk_size(handle, 65536),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_set_output_pack_compression_type(handle, 3),
            0
        );
        assert_eq!((*handle).output_solid_chunk_size, 65536);
        assert_eq!(wim::ffi::wimlib_set_output_pack_chunk_size(handle, 0), 0);
        assert_eq!((*handle).output_solid_chunk_size, 67108864);
        let mut after = WimInfo::default();
        wimlib_get_wim_info(handle, &mut after);
        assert_eq!(before, after);
        wimlib_free(handle);
    }
}

#[test]
fn integrity_presence_uses_header_offset_not_resource_size() {
    let mut handle = std::ptr::null_mut();
    // SAFETY: Handle is owned by this test and used sequentially.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        (*handle).header.integrity_table.offset_in_wim = 208;
        let mut info = WimInfo::default();
        wimlib_get_wim_info(handle, &mut info);
        assert_eq!(info.flags & 1, 1);
        (*handle).header.integrity_table.offset_in_wim = 0;
        (*handle).header.integrity_table.size_in_wim = 100;
        wimlib_get_wim_info(handle, &mut info);
        assert_eq!(info.flags & 1, 0);
        wimlib_free(handle);
    }
}
