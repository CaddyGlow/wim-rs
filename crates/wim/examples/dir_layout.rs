//! Emit public Rust callback layouts for comparison with unchanged C headers.
use std::mem::{offset_of, size_of};
use wim::ffi::{WimDirEntry, WimObjectId, WimStreamEntry, WimTimespec};
fn main() {
    println!(
        "dir.size={}\nstream.size={}\nstream.resource={}\nstream.reserved={}\nobject.size={}\ntime.size={}\ntime.nsec={}",
        size_of::<WimDirEntry>(),
        size_of::<WimStreamEntry>(),
        offset_of!(WimStreamEntry, resource),
        offset_of!(WimStreamEntry, reserved),
        size_of::<WimObjectId>(),
        size_of::<WimTimespec>(),
        offset_of!(WimTimespec, tv_nsec)
    );
    macro_rules! off {($($field:ident),*)=>{$(println!("dir.{}={}",stringify!($field),offset_of!(WimDirEntry,$field));)*};}
    off!(
        filename,
        dos_name,
        full_path,
        depth,
        security_descriptor,
        security_descriptor_size,
        attributes,
        reparse_tag,
        num_links,
        num_named_streams,
        hard_link_group_id,
        creation_time,
        last_write_time,
        last_access_time,
        unix_uid,
        unix_gid,
        unix_mode,
        unix_rdev,
        object_id,
        creation_time_high,
        last_write_time_high,
        last_access_time_high,
        reserved2,
        reserved,
        streams
    );
}
