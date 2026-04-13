use std::path::PathBuf;

use crate::error::BenchError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairResult {
    pub cover: PathBuf,
    pub stego: PathBuf,
    pub mse: f64,
    pub psnr_db: f64,
    pub ssim: f64,
    pub phash_hamming: u32,
    pub dhash_hamming: u32,
    pub file_size_delta: i64,
    pub steganalyzer_scores: Vec<(String, f64)>,
    pub embed_ms: Option<f64>,
    pub extract_ms: Option<f64>,
    pub roundtrip_ok: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchSummary {
    pub pair_count: usize,
    pub mean_mse: f64,
    pub mean_psnr_db: f64,
    pub mean_ssim: f64,
    pub mean_phash_hamming: f64,
    pub mean_dhash_hamming: f64,
    pub p50_ssim: f64,
    pub p90_ssim: f64,
    pub pairs: Vec<PairResult>,
}

impl BenchSummary {
    pub fn from_pairs(pairs: Vec<PairResult>) -> Self {
        let n = pairs.len();
        if n == 0 {
            return Self {
                pair_count: 0,
                mean_mse: 0.0,
                mean_psnr_db: 0.0,
                mean_ssim: 0.0,
                mean_phash_hamming: 0.0,
                mean_dhash_hamming: 0.0,
                p50_ssim: 0.0,
                p90_ssim: 0.0,
                pairs,
            };
        }

        let mean_mse = pairs.iter().map(|p| p.mse).sum::<f64>() / n as f64;
        let mean_psnr_db = pairs.iter().map(|p| p.psnr_db).sum::<f64>() / n as f64;
        let mean_ssim = pairs.iter().map(|p| p.ssim).sum::<f64>() / n as f64;
        let mean_phash_hamming =
            pairs.iter().map(|p| p.phash_hamming as f64).sum::<f64>() / n as f64;
        let mean_dhash_hamming =
            pairs.iter().map(|p| p.dhash_hamming as f64).sum::<f64>() / n as f64;

        let mut ssim_sorted: Vec<f64> = pairs.iter().map(|p| p.ssim).collect();
        ssim_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50_ssim = percentile(&ssim_sorted, 50.0);
        let p90_ssim = percentile(&ssim_sorted, 90.0);

        Self {
            pair_count: n,
            mean_mse,
            mean_psnr_db,
            mean_ssim,
            mean_phash_hamming,
            mean_dhash_hamming,
            p50_ssim,
            p90_ssim,
            pairs,
        }
    }

    pub fn to_json(&self) -> Result<String, BenchError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# Phantasm Bench Report\n\n");
        md.push_str("## Summary\n\n");
        md.push_str("| Metric | Value |\n|--------|-------|\n");
        md.push_str(&format!("| Pair count | {} |\n", self.pair_count));
        md.push_str(&format!("| Mean MSE | {:.4} |\n", self.mean_mse));
        md.push_str(&format!("| Mean PSNR (dB) | {:.2} |\n", self.mean_psnr_db));
        md.push_str(&format!("| Mean SSIM | {:.4} |\n", self.mean_ssim));
        md.push_str(&format!(
            "| Mean pHash hamming | {:.2} |\n",
            self.mean_phash_hamming
        ));
        md.push_str(&format!(
            "| Mean dHash hamming | {:.2} |\n",
            self.mean_dhash_hamming
        ));
        md.push_str(&format!("| p50 SSIM | {:.4} |\n", self.p50_ssim));
        md.push_str(&format!("| p90 SSIM | {:.4} |\n", self.p90_ssim));
        md.push_str("\n## Pairs\n\n");
        md.push_str("| Cover | Stego | MSE | PSNR (dB) | SSIM | pHash Δ | dHash Δ | Size Δ |\n");
        md.push_str("|-------|-------|-----|-----------|------|---------|---------|--------|\n");
        for p in &self.pairs {
            md.push_str(&format!(
                "| {} | {} | {:.4} | {:.2} | {:.4} | {} | {} | {} |\n",
                p.cover.file_name().unwrap_or_default().to_string_lossy(),
                p.stego.file_name().unwrap_or_default().to_string_lossy(),
                p.mse,
                p.psnr_db,
                p.ssim,
                p.phash_hamming,
                p.dhash_hamming,
                p.file_size_delta,
            ));
        }
        md
    }
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (pct / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
