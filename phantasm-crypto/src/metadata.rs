use crate::{CryptoError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentType {
    Raw = 0,
    Utf8 = 1,
    File = 2,
}

impl ContentType {
    fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Raw),
            1 => Ok(Self::Utf8),
            2 => Ok(Self::File),
            _ => Err(CryptoError::InvalidData(format!(
                "unknown content type: {v}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadMetadata {
    pub filename: Option<String>,
    pub payload_len: u64,
    pub content_type: ContentType,
    pub version: u16,
}

impl PayloadMetadata {
    /// Wire format: [version: u16 LE][content_type: u8][payload_len: u64 LE][filename_len: u16 LE][filename: bytes]
    pub fn to_bytes(&self) -> Vec<u8> {
        let filename_bytes = self.filename.as_deref().unwrap_or("").as_bytes();
        let filename_len = filename_bytes.len() as u16;

        let mut buf = Vec::with_capacity(2 + 1 + 8 + 2 + filename_bytes.len());
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.push(self.content_type.clone() as u8);
        buf.extend_from_slice(&self.payload_len.to_le_bytes());
        buf.extend_from_slice(&filename_len.to_le_bytes());
        buf.extend_from_slice(filename_bytes);
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 13 {
            return Err(CryptoError::InvalidData("metadata too short".into()));
        }
        let version = u16::from_le_bytes([data[0], data[1]]);
        let content_type = ContentType::from_u8(data[2])?;
        let payload_len = u64::from_le_bytes(data[3..11].try_into().unwrap());
        let filename_len = u16::from_le_bytes([data[11], data[12]]) as usize;

        let total = 13 + filename_len;
        if data.len() < total {
            return Err(CryptoError::InvalidData("metadata truncated".into()));
        }

        let filename = if filename_len == 0 {
            None
        } else {
            Some(
                std::str::from_utf8(&data[13..13 + filename_len])
                    .map_err(|_| CryptoError::InvalidData("invalid UTF-8 in filename".into()))?
                    .to_owned(),
            )
        };

        Ok((
            Self {
                filename,
                payload_len,
                content_type,
                version,
            },
            total,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip_no_filename() {
        let m = PayloadMetadata {
            filename: None,
            payload_len: 1024,
            content_type: ContentType::Raw,
            version: 1,
        };
        let bytes = m.to_bytes();
        let (m2, consumed) = PayloadMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(m, m2);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn metadata_roundtrip_with_filename() {
        let m = PayloadMetadata {
            filename: Some("secret.txt".into()),
            payload_len: 42,
            content_type: ContentType::File,
            version: 1,
        };
        let bytes = m.to_bytes();
        let (m2, consumed) = PayloadMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(m, m2);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn metadata_roundtrip_utf8_type() {
        let m = PayloadMetadata {
            filename: None,
            payload_len: 512,
            content_type: ContentType::Utf8,
            version: 1,
        };
        let bytes = m.to_bytes();
        let (m2, _) = PayloadMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(m, m2);
    }
}
