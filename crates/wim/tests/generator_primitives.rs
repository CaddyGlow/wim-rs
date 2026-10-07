//! Original unchanged-source generator goldens, independent of native capture.
#![cfg(feature = "test-support")]
use sha1::{Digest, Sha1};
use wim::engine::test_support::{primitives, random::Random};

#[test]
fn bounded_generators_preserve_original_bytes_and_random_consumption() {
    for row in include_str!("fixtures/generator-primitives.tsv").lines() {
        let fields: Vec<_> = row.split('\t').collect();
        let seed = fields[0].parse().unwrap();
        let size = fields[2].parse().unwrap();
        let mut random = Random::Local(seed);
        let bytes = match fields[1] {
            "filename" => primitives::filename(&mut random, size).unwrap(),
            "short" => primitives::short_name(&mut random).unwrap(),
            "timestamp" => primitives::timestamp(&mut random).to_le_bytes().to_vec(),
            "security" => primitives::security_descriptor(&mut random).unwrap(),
            "data" => primitives::data(&mut random, size).unwrap(),
            _ => panic!("unknown golden kind"),
        };
        assert_eq!(bytes.len(), fields[3].parse::<usize>().unwrap(), "{row}");
        assert_eq!(format!("{:x}", Sha1::digest(&bytes)), fields[4], "{row}");
        assert_eq!(format!("{:08x}", random.next_u32()), fields[5], "{row}");
    }
}
