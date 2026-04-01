use sha2::{Digest, Sha256};
use std::io::{self, Read};

pub const HASH_BUF_SIZE: usize = 262144; // 256KB

pub fn hash_reader(reader: &mut dyn Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_BUF_SIZE];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Hash a file at the given path
pub fn hash_file(path: &std::path::Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    hash_reader(&mut file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_hash_reader_known_digest() {
        // SHA-256 of "hello world" (no newline)
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let mut cursor = Cursor::new(b"hello world");
        let result = hash_reader(&mut cursor).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_hash_reader_empty() {
        // SHA-256 of empty input
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let mut cursor = Cursor::new(b"");
        let result = hash_reader(&mut cursor).unwrap();
        assert_eq!(result, expected);
    }
}
