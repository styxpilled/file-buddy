//! Content identity. A rename or a move changes the path, not the hash, so an
//! expensive stage never runs twice for the same bytes.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};

/// How much is read from each end of the file.
const WINDOW: u64 = 1024 * 1024;

/// blake3 over the size, the first 1 MB and the last 1 MB, as 64 hexadecimal
/// characters.
pub fn file_hash(path: &Path, size: u64) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());

    let head = size.min(WINDOW);
    copy_window(&mut file, 0, head, &mut hasher)?;

    // Clipped so the head and the tail never cover the same bytes twice.
    let tail_start = size.saturating_sub(WINDOW).max(head);
    if tail_start < size {
        copy_window(&mut file, tail_start, size - tail_start, &mut hasher)?;
    }

    Ok(hasher.finalize().to_hex().to_string())
}

fn copy_window(
    file: &mut File,
    offset: u64,
    length: u64,
    hasher: &mut blake3::Hasher,
) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0u8; length as usize];
    let mut filled = 0;
    while filled < buffer.len() {
        let read = file.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    hasher.update(&buffer[..filled]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        File::create(&path).unwrap().write_all(bytes).unwrap();
        path
    }

    #[test]
    fn same_bytes_hash_the_same_under_any_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.bin", b"hello world");
        let b = write(dir.path(), "b.bin", b"hello world");
        assert_eq!(file_hash(&a, 11).unwrap(), file_hash(&b, 11).unwrap());
    }

    #[test]
    fn a_changed_middle_of_a_large_file_is_invisible() {
        let dir = tempfile::tempdir().unwrap();
        let size = (WINDOW * 3) as usize;
        let mut bytes = vec![7u8; size];
        let a = write(dir.path(), "a.bin", &bytes);
        bytes[size / 2] = 9;
        let b = write(dir.path(), "b.bin", &bytes);
        // Known and accepted: only both ends and the size are read.
        assert_eq!(
            file_hash(&a, size as u64).unwrap(),
            file_hash(&b, size as u64).unwrap()
        );
    }

    #[test]
    fn a_changed_end_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let size = (WINDOW * 3) as usize;
        let mut bytes = vec![7u8; size];
        let a = write(dir.path(), "a.bin", &bytes);
        bytes[size - 1] = 9;
        let b = write(dir.path(), "b.bin", &bytes);
        assert_ne!(
            file_hash(&a, size as u64).unwrap(),
            file_hash(&b, size as u64).unwrap()
        );
    }

    #[test]
    fn the_size_is_part_of_the_identity() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.bin", b"abc");
        let b = write(dir.path(), "b.bin", b"abcd");
        assert_ne!(file_hash(&a, 3).unwrap(), file_hash(&b, 4).unwrap());
    }
}
