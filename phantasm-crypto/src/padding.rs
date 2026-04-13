use rand::rngs::OsRng;
use rand::RngCore;

use crate::{CryptoError, Result};

pub const BLOCK_SIZES: &[usize] = &[256, 1024, 4096, 16384, 65536, 262144];

/// Pads content to the smallest fitting block size.
/// Wire format: [content_len: u32 LE][content][random padding...]
pub fn pad(content: &[u8]) -> Result<Vec<u8>> {
    let content_len = content.len();
    let total_needed = 4 + content_len;

    let block_size = BLOCK_SIZES
        .iter()
        .copied()
        .find(|&b| b >= total_needed)
        .ok_or(CryptoError::PayloadTooLarge)?;

    let mut buf = Vec::with_capacity(block_size);
    buf.extend_from_slice(&(content_len as u32).to_le_bytes());
    buf.extend_from_slice(content);

    let pad_start = buf.len();
    buf.resize(block_size, 0u8);
    OsRng.fill_bytes(&mut buf[pad_start..]);

    Ok(buf)
}

/// Strips padding, returning the original content.
pub fn unpad(padded: &[u8]) -> Result<Vec<u8>> {
    if padded.len() < 4 {
        return Err(CryptoError::InvalidData("padded data too short".into()));
    }
    let content_len = u32::from_le_bytes(padded[..4].try_into().unwrap()) as usize;
    if 4 + content_len > padded.len() {
        return Err(CryptoError::InvalidData(
            "content length exceeds padded buffer".into(),
        ));
    }
    Ok(padded[4..4 + content_len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combined(meta_len: usize, payload_len: usize) -> Vec<u8> {
        vec![0u8; meta_len + payload_len]
    }

    #[test]
    fn padding_block_selection() {
        // 100 bytes content → 4+100=104 → fits in 256
        let padded = pad(&combined(0, 100)).unwrap();
        assert_eq!(padded.len(), 256);

        // 500 bytes → 4+500=504 → fits in 1024
        let padded = pad(&combined(0, 500)).unwrap();
        assert_eq!(padded.len(), 1024);

        // 5000 bytes → 4+5000=5004 → fits in 16384 (4096 < 5004 ≤ 16384)
        let padded = pad(&combined(0, 5000)).unwrap();
        assert_eq!(padded.len(), 16384);

        // 100 KiB = 102400 bytes → 4+102400=102404 → fits in 262144
        let padded = pad(&combined(0, 102400)).unwrap();
        assert_eq!(padded.len(), 262144);
    }

    #[test]
    fn padding_too_large() {
        // 263000 bytes → 4+263000 > 262144 → error
        let content = vec![0u8; 263000];
        assert!(matches!(pad(&content), Err(CryptoError::PayloadTooLarge)));
    }

    #[test]
    fn padding_roundtrip() {
        let original = b"hello world this is a test payload";
        let padded = pad(original).unwrap();
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn padding_strips_exact_length() {
        for &len in &[1usize, 100, 500, 5000, 102400] {
            let content = vec![0xabu8; len];
            let padded = pad(&content).unwrap();
            let recovered = unpad(&padded).unwrap();
            assert_eq!(recovered.len(), len);
        }
    }
}
