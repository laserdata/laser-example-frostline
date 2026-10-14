use sha2::{Digest, Sha256};
use std::fmt::Write;

/// An order-sensitive digest of selected records. Each part is length-prefixed, so no two sequences collide.
#[derive(Clone, Debug, Default)]
pub struct RollingDigest(Sha256);

impl RollingDigest {
    pub fn push(&mut self, event_id: u64, sequence: u64, payload: &[u8]) {
        for part in [
            event_id.to_le_bytes().as_slice(),
            sequence.to_le_bytes().as_slice(),
            payload,
        ] {
            self.0.update((part.len() as u64).to_le_bytes());
            self.0.update(part);
        }
    }

    pub fn finish(self) -> String {
        hex(&self.0.finalize())
    }
}

/// Lowercase hex of `bytes`, the form every digest takes in reports and run files.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            write!(text, "{byte:02x}").expect("writing to a string cannot fail");
            text
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_same_records_in_another_order_when_digested_then_should_differ() {
        let mut first = RollingDigest::default();
        first.push(1, 1, b"a");
        first.push(2, 2, b"b");
        let mut second = RollingDigest::default();
        second.push(2, 2, b"b");
        second.push(1, 1, b"a");
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn given_payloads_split_at_another_boundary_when_digested_then_should_differ() {
        let mut first = RollingDigest::default();
        first.push(1, 1, b"ab");
        first.push(2, 2, b"c");
        let mut second = RollingDigest::default();
        second.push(1, 1, b"a");
        second.push(2, 2, b"bc");
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn given_nothing_when_digested_then_should_be_the_sha256_of_empty_input() {
        assert_eq!(
            RollingDigest::default().finish(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
