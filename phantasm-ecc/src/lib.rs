// Reed-Solomon library choice: reed-solomon-erasure v6.x
// Rationale: widely used, actively maintained, supports GF(2^8) with a
// clean shard-based API that maps directly to our data/parity shard model.
// reed-solomon-simd is faster but requires nightly SIMD features on some
// targets; reed-solomon-novelpoly has a less ergonomic API for our use case.

use reed_solomon_erasure::galois_8::ReedSolomon;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum EccError {
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    #[error("input length mismatch: expected {expected}, got {got}")]
    LengthMismatch { expected: usize, got: usize },
    #[error("unrecoverable corruption: too many shards lost or corrupted")]
    UnrecoverableCorruption,
    #[error("internal RS error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct EccParams {
    pub data_shards: usize,
    pub parity_shards: usize,
    pub shard_size: usize,
    channel: Option<String>,
}

impl EccParams {
    pub fn new(data_shards: usize, parity_shards: usize, shard_size: usize) -> Self {
        Self {
            data_shards,
            parity_shards,
            shard_size,
            channel: None,
        }
    }

    pub fn redundancy_ratio(&self) -> f64 {
        if self.data_shards == 0 {
            return 0.0;
        }
        self.parity_shards as f64 / self.data_shards as f64
    }

    pub fn channel_name(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    pub fn for_channel(channel: &str) -> Option<Self> {
        let (data_shards, parity_shards) = match channel {
            "lossless" => (255, 0),
            "signal" => (243, 12),
            "twitter" => (222, 33),
            "facebook" => (204, 51),
            "whatsapp-photo" => (189, 66),
            "whatsapp-doc" => (255, 0),
            "instagram" => (204, 51),
            "generic-75" => (229, 26),
            _ => return None,
        };
        Some(Self {
            data_shards,
            parity_shards,
            shard_size: 64,
            channel: Some(channel.to_string()),
        })
    }
}

pub struct Encoder {
    params: EccParams,
    inner: Option<ReedSolomon>,
}

pub struct Decoder {
    params: EccParams,
    inner: Option<ReedSolomon>,
}

impl Encoder {
    pub fn new(params: EccParams) -> Result<Self, EccError> {
        if params.shard_size == 0 {
            return Err(EccError::InvalidParams("shard_size must be > 0".into()));
        }
        if params.data_shards == 0 {
            return Err(EccError::InvalidParams("data_shards must be > 0".into()));
        }
        let inner = if params.parity_shards > 0 {
            Some(
                ReedSolomon::new(params.data_shards, params.parity_shards)
                    .map_err(|e| EccError::Internal(e.to_string()))?,
            )
        } else {
            None
        };
        Ok(Self { params, inner })
    }

    /// Output layout: [len: u32 LE][data bytes + zero padding][parity bytes]
    pub fn encode(&self, payload: &[u8]) -> Result<Vec<u8>, EccError> {
        let original_len = payload.len() as u32;
        let block_size = self.params.data_shards * self.params.shard_size;

        // Build data section: 4-byte length prefix + payload, padded to block boundary
        let prefix = original_len.to_le_bytes();
        let content_len = 4 + payload.len();
        let padded_len = if content_len.is_multiple_of(block_size) {
            content_len
        } else {
            content_len + (block_size - content_len % block_size)
        };

        let mut data = vec![0u8; padded_len];
        data[..4].copy_from_slice(&prefix);
        data[4..4 + payload.len()].copy_from_slice(payload);

        if self.inner.is_none() {
            // Zero-parity short-circuit
            return Ok(data);
        }

        let rs = self.inner.as_ref().unwrap();
        let num_blocks = padded_len / block_size;
        let parity_len = num_blocks * self.params.parity_shards * self.params.shard_size;
        let mut output = Vec::with_capacity(padded_len + parity_len);
        output.extend_from_slice(&data);
        output.resize(padded_len + parity_len, 0u8);

        // Encode block by block
        for block_idx in 0..num_blocks {
            let data_offset = block_idx * block_size;
            let parity_offset =
                padded_len + block_idx * self.params.parity_shards * self.params.shard_size;

            let mut shards: Vec<Vec<u8>> = (0..self.params.data_shards)
                .map(|s| {
                    let start = data_offset + s * self.params.shard_size;
                    data[start..start + self.params.shard_size].to_vec()
                })
                .collect();
            for _ in 0..self.params.parity_shards {
                shards.push(vec![0u8; self.params.shard_size]);
            }

            rs.encode(&mut shards)
                .map_err(|e| EccError::Internal(e.to_string()))?;

            for s in 0..self.params.parity_shards {
                let start = parity_offset + s * self.params.shard_size;
                output[start..start + self.params.shard_size]
                    .copy_from_slice(&shards[self.params.data_shards + s]);
            }
        }

        Ok(output)
    }
}

impl Decoder {
    pub fn new(params: EccParams) -> Result<Self, EccError> {
        if params.shard_size == 0 {
            return Err(EccError::InvalidParams("shard_size must be > 0".into()));
        }
        if params.data_shards == 0 {
            return Err(EccError::InvalidParams("data_shards must be > 0".into()));
        }
        let inner = if params.parity_shards > 0 {
            Some(
                ReedSolomon::new(params.data_shards, params.parity_shards)
                    .map_err(|e| EccError::Internal(e.to_string()))?,
            )
        } else {
            None
        };
        Ok(Self { params, inner })
    }

    pub fn decode(&self, encoded: &[u8], erasures: Option<&[usize]>) -> Result<Vec<u8>, EccError> {
        let block_size = self.params.data_shards * self.params.shard_size;

        if self.inner.is_none() {
            // Zero-parity: data section is the entire encoded buffer
            if encoded.len() < 4 {
                return Err(EccError::LengthMismatch {
                    expected: 4,
                    got: encoded.len(),
                });
            }
            let original_len = u32::from_le_bytes(encoded[..4].try_into().unwrap()) as usize;
            let payload = encoded[4..].to_vec();
            if original_len > payload.len() {
                return Err(EccError::LengthMismatch {
                    expected: original_len,
                    got: payload.len(),
                });
            }
            return Ok(payload[..original_len].to_vec());
        }

        let parity_block_size = self.params.parity_shards * self.params.shard_size;
        let total_block = block_size + parity_block_size;

        if !encoded.len().is_multiple_of(total_block) {
            return Err(EccError::LengthMismatch {
                expected: (encoded.len() / total_block + 1) * total_block,
                got: encoded.len(),
            });
        }

        let num_blocks = encoded.len() / total_block;
        let data_section_len = num_blocks * block_size;
        let rs = self.inner.as_ref().unwrap();

        let mut recovered_data = vec![0u8; data_section_len];

        for block_idx in 0..num_blocks {
            let data_offset = block_idx * block_size;
            let parity_offset = data_section_len + block_idx * parity_block_size;

            // Convert global shard erasures to per-block shard indices
            let total_shards = self.params.data_shards + self.params.parity_shards;
            let shard_start = block_idx * total_shards;

            let mut shards: Vec<Option<Vec<u8>>> = (0..self.params.data_shards)
                .map(|s| {
                    let start = data_offset + s * self.params.shard_size;
                    Some(encoded[start..start + self.params.shard_size].to_vec())
                })
                .collect();
            for s in 0..self.params.parity_shards {
                let start = parity_offset + s * self.params.shard_size;
                shards.push(Some(
                    encoded[start..start + self.params.shard_size].to_vec(),
                ));
            }

            // Mark erasures
            if let Some(erased) = erasures {
                for &global_idx in erased {
                    if global_idx >= shard_start && global_idx < shard_start + total_shards {
                        let local_idx = global_idx - shard_start;
                        if local_idx < shards.len() {
                            shards[local_idx] = None;
                        }
                    }
                }
            }

            let erasure_count = shards.iter().filter(|s| s.is_none()).count();
            if erasure_count > self.params.parity_shards {
                return Err(EccError::UnrecoverableCorruption);
            }

            rs.reconstruct(&mut shards)
                .map_err(|_| EccError::UnrecoverableCorruption)?;

            for (s, shard) in shards.iter().enumerate().take(self.params.data_shards) {
                let start = data_offset + s * self.params.shard_size;
                recovered_data[start..start + self.params.shard_size]
                    .copy_from_slice(shard.as_ref().unwrap());
            }
        }

        if recovered_data.len() < 4 {
            return Err(EccError::LengthMismatch {
                expected: 4,
                got: recovered_data.len(),
            });
        }
        let original_len = u32::from_le_bytes(recovered_data[..4].try_into().unwrap()) as usize;
        let payload = &recovered_data[4..];
        if original_len > payload.len() {
            return Err(EccError::LengthMismatch {
                expected: original_len,
                got: payload.len(),
            });
        }
        Ok(payload[..original_len].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    fn twitter_params() -> EccParams {
        EccParams::for_channel("twitter").unwrap()
    }

    fn facebook_params() -> EccParams {
        EccParams::for_channel("facebook").unwrap()
    }

    fn signal_params() -> EccParams {
        EccParams::for_channel("signal").unwrap()
    }

    fn lossless_params() -> EccParams {
        EccParams::for_channel("lossless").unwrap()
    }

    #[test]
    fn test_roundtrip_1kb_twitter() {
        let mut rng = StdRng::seed_from_u64(42);
        let payload: Vec<u8> = (0..1024).map(|_| rng.gen()).collect();
        let params = twitter_params();
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params).unwrap();
        let encoded = encoder.encode(&payload).unwrap();
        let decoded = decoder.decode(&encoded, None).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_roundtrip_short_payload() {
        let payload = b"0123456789".to_vec();
        let params = twitter_params();
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params).unwrap();
        let encoded = encoder.encode(&payload).unwrap();
        let decoded = decoder.decode(&encoded, None).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_roundtrip_64kb() {
        let mut rng = StdRng::seed_from_u64(99);
        let payload: Vec<u8> = (0..65536).map(|_| rng.gen()).collect();
        let params = twitter_params();
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params).unwrap();
        let encoded = encoder.encode(&payload).unwrap();
        let decoded = decoder.decode(&encoded, None).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_lossless_zero_parity() {
        let mut rng = StdRng::seed_from_u64(7);
        let payload: Vec<u8> = (0..512).map(|_| rng.gen()).collect();
        let params = lossless_params();
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params).unwrap();
        let encoded = encoder.encode(&payload).unwrap();
        let decoded = decoder.decode(&encoded, None).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_recovery_from_missing_shards() {
        let mut rng = StdRng::seed_from_u64(13);
        let payload: Vec<u8> = (0..1024).map(|_| rng.gen()).collect();
        let params = twitter_params(); // parity_shards = 33
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params.clone()).unwrap();
        let encoded = encoder.encode(&payload).unwrap();

        // Calculate total shards across all blocks
        let block_size = params.data_shards * params.shard_size;
        let parity_block_size = params.parity_shards * params.shard_size;
        let total_block = block_size + parity_block_size;
        let num_blocks = encoded.len() / total_block;
        let total_shards = num_blocks * (params.data_shards + params.parity_shards);

        // Pick 15 random shard indices to erase (< 33 per block, so recoverable)
        // Distribute evenly: at most floor(15/num_blocks) per block
        let mut erasures: Vec<usize> = Vec::new();
        let per_block = std::cmp::min(15 / num_blocks, params.parity_shards - 1);
        for b in 0..num_blocks {
            let shards_per_block = params.data_shards + params.parity_shards;
            let block_start = b * shards_per_block;
            for i in 0..per_block {
                erasures.push(block_start + i);
            }
        }
        // Ensure we don't exceed recoverable
        erasures.truncate(total_shards.min(15));

        let decoded = decoder.decode(&encoded, Some(&erasures)).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_recovery_from_corrupted_shards() {
        let mut rng = StdRng::seed_from_u64(17);
        let payload: Vec<u8> = (0..1024).map(|_| rng.gen()).collect();
        let params = facebook_params(); // parity_shards = 51
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params.clone()).unwrap();
        let mut encoded = encoder.encode(&payload).unwrap();

        let block_size = params.data_shards * params.shard_size;
        let parity_block_size = params.parity_shards * params.shard_size;
        let total_block = block_size + parity_block_size;
        let num_blocks = encoded.len() / total_block;

        // Flip bits in 20 shards, spread across blocks (mark as erasures so decoder knows)
        let mut erasures: Vec<usize> = Vec::new();
        let per_block = std::cmp::min(20 / num_blocks, params.parity_shards - 1);
        for b in 0..num_blocks {
            let shards_per_block = params.data_shards + params.parity_shards;
            let block_start = b * shards_per_block;
            for i in 0..per_block {
                let shard_idx = block_start + i;
                // Find offset in encoded buffer for this shard
                let offset = if i < params.data_shards {
                    b * block_size + i * params.shard_size
                } else {
                    num_blocks * block_size
                        + b * parity_block_size
                        + (i - params.data_shards) * params.shard_size
                };
                encoded[offset] ^= 0xFF;
                erasures.push(shard_idx);
            }
        }

        let decoded = decoder.decode(&encoded, Some(&erasures)).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_unrecoverable_corruption() {
        let mut rng = StdRng::seed_from_u64(31);
        let payload: Vec<u8> = (0..1024).map(|_| rng.gen()).collect();
        let params = signal_params(); // parity_shards = 12
        let encoder = Encoder::new(params.clone()).unwrap();
        let decoder = Decoder::new(params.clone()).unwrap();
        let encoded = encoder.encode(&payload).unwrap();

        // Mark 20 erasures in first block (> parity_shards=12) — unrecoverable
        let mut erasures: Vec<usize> = Vec::new();
        for i in 0..20 {
            erasures.push(i); // all in block 0
        }

        let result = decoder.decode(&encoded, Some(&erasures));
        assert!(matches!(result, Err(EccError::UnrecoverableCorruption)));
    }

    #[test]
    fn test_per_channel_lookup() {
        let fb = EccParams::for_channel("facebook").unwrap();
        assert_eq!(fb.parity_shards, 51);

        let none = EccParams::for_channel("nonsense");
        assert!(none.is_none());
    }

    #[test]
    fn test_redundancy_ratio() {
        let fb = EccParams::for_channel("facebook").unwrap();
        let ratio = fb.redundancy_ratio();
        assert!(
            (ratio - 0.25).abs() < 0.01,
            "facebook ratio {ratio} not ≈ 0.25"
        );

        let ll = EccParams::for_channel("lossless").unwrap();
        assert_eq!(ll.redundancy_ratio(), 0.0);
    }
}
