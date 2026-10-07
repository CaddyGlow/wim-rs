use wim_format::{
    Compression, ParseError,
    archive::Archive,
    image_ops::{delete_image, select_images},
    repack::WriteOptions,
};
const FILE: &[u8] = include_bytes!("fixtures/xpress-resource.wim");
fn options() -> WriteOptions {
    WriteOptions {
        compression: Compression::None,
        chunk_size: 0,
        integrity: true,
    }
}
#[test]
fn selected_image_preserves_xml_metadata_and_content() {
    let source = Archive::open(FILE).unwrap();
    let bytes = select_images(&source, &[1], options()).unwrap();
    let output = Archive::open(&bytes).unwrap();
    assert_eq!(
        output.read_metadata(1).unwrap(),
        source.read_metadata(1).unwrap()
    );
    assert_eq!(output.xml().unwrap().name(1), source.xml().unwrap().name(1));
    for blob in &output.lookup.blobs {
        assert_eq!(
            output.read_blob(&blob.hash).unwrap(),
            source.read_blob(&blob.hash).unwrap()
        );
    }
}
#[test]
fn invalid_selection_rejected_without_output() {
    let source = Archive::open(FILE).unwrap();
    for images in [&[0][..], &[2][..]] {
        assert_eq!(
            select_images(&source, images, options()).unwrap_err(),
            ParseError::InvalidImage
        );
    }
    assert_eq!(
        select_images(&source, &[1, 1], options()).unwrap_err(),
        ParseError::InvalidParam
    );
    let empty = delete_image(&source, 1, options()).unwrap();
    assert_eq!(Archive::open(&empty).unwrap().header.image_count, 0);
}

#[test]
fn export_rejects_name_collision_without_changing_destination() {
    let source = Archive::open(FILE).unwrap();
    assert_eq!(
        wim_format::image_ops::export_images(&source, &source, &[1], options()).unwrap_err(),
        ParseError::ImageNameCollision
    );
    assert_eq!(source.header.image_count, 1);
}

#[test]
fn export_into_empty_destination_preserves_content_and_deduplicates() {
    let source = Archive::open(FILE).unwrap();
    let empty = select_images(&source, &[], options()).unwrap();
    let destination = Archive::open(&empty).unwrap();
    let bytes =
        wim_format::image_ops::export_images(&destination, &source, &[1], options()).unwrap();
    let output = Archive::open(&bytes).unwrap();
    assert_eq!(output.header.image_count, 1);
    assert_eq!(
        output.read_metadata(1).unwrap(),
        source.read_metadata(1).unwrap()
    );
    for blob in &output.lookup.blobs {
        assert_eq!(
            output.read_blob(&blob.hash).unwrap(),
            source.read_blob(&blob.hash).unwrap()
        );
    }
}
