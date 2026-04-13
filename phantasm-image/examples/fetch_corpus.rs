use image::codecs::jpeg::JpegEncoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct ImageEntry {
    path: String,
    source_url: String,
    seed: String,
    dimensions: [u32; 2],
    quality_factor: u8,
    size_bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct Manifest {
    source: String,
    fetched_at: String,
    total_count: usize,
    by_qf: HashMap<String, usize>,
    by_dimension: HashMap<String, usize>,
    images: Vec<ImageEntry>,
}

struct Slot {
    seed_num: u32,
    qf: u8,
    dim_label: &'static str,
    width: u32,
    height: u32,
    file_num: usize,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn main() {
    let corpus_root = PathBuf::from(
        std::env::var("CORPUS_DIR")
            .unwrap_or_else(|_| "/Users/dylan/Developer/phantasm/research-corpus".to_string()),
    );

    let qfs: &[u8] = &[75, 85, 90];
    let dims: &[(&str, u32, u32)] = &[("512", 512, 512), ("1024", 1024, 1024), ("720", 720, 680)];
    let per_bucket: usize = 22;

    let mut slots: Vec<Slot> = Vec::with_capacity(198);
    let mut seed_counter = 1u32;
    for &qf in qfs {
        for &(label, w, h) in dims {
            for file_num in 1..=per_bucket {
                slots.push(Slot {
                    seed_num: seed_counter,
                    qf,
                    dim_label: label,
                    width: w,
                    height: h,
                    file_num,
                });
                seed_counter += 1;
            }
        }
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client");

    let mut entries: Vec<ImageEntry> = Vec::new();
    let mut skipped: Vec<u32> = Vec::new();
    let start = Instant::now();
    let total = slots.len();

    for (idx, slot) in slots.iter().enumerate() {
        let seed = format!("phantasm-{:04}", slot.seed_num);
        let url = format!(
            "https://picsum.photos/seed/{}/{}/{}",
            seed, slot.width, slot.height
        );

        let dir = corpus_root
            .join(format!("qf{}", slot.qf))
            .join(slot.dim_label);
        let filename = format!("{:04}.jpg", slot.file_num);
        let out_path = dir.join(&filename);
        let rel_path = format!("qf{}/{}/{}", slot.qf, slot.dim_label, filename);

        if (idx + 1) % 20 == 0 || idx == 0 {
            println!("[{}/{}] fetching {} -> {}", idx + 1, total, seed, rel_path);
        }

        // Fetch with retry on 429
        let bytes = 'fetch: {
            loop {
                match client.get(&url).send() {
                    Ok(resp) if resp.status() == 429 => {
                        eprintln!("  rate-limited on {seed}, sleeping 500ms");
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    Ok(resp) if resp.status().is_success() => match resp.bytes() {
                        Ok(b) => break 'fetch Some(b),
                        Err(e) => {
                            eprintln!("  read error for {seed}: {e}, skipping");
                            skipped.push(slot.seed_num);
                            break 'fetch None;
                        }
                    },
                    Ok(resp) => {
                        eprintln!("  HTTP {} for {seed}, skipping", resp.status());
                        skipped.push(slot.seed_num);
                        break 'fetch None;
                    }
                    Err(e) => {
                        eprintln!("  request error for {seed}: {e}, skipping");
                        skipped.push(slot.seed_num);
                        break 'fetch None;
                    }
                }
            }
        };

        let bytes = match bytes {
            Some(b) => b,
            None => continue,
        };

        // Decode and re-encode at target QF
        let img = match image::load_from_memory(&bytes) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("  decode error for {seed}: {e}, skipping");
                skipped.push(slot.seed_num);
                continue;
            }
        };

        let mut out_buf: Vec<u8> = Vec::new();
        let encoder = JpegEncoder::new_with_quality(&mut out_buf, slot.qf);
        if let Err(e) = img.write_with_encoder(encoder) {
            eprintln!("  encode error for {seed}: {e}, skipping");
            skipped.push(slot.seed_num);
            continue;
        }

        fs::create_dir_all(&dir).expect("failed to create output dir");
        fs::write(&out_path, &out_buf).expect("failed to write image");

        let size_bytes = out_buf.len() as u64;
        let sha256 = sha256_hex(&out_buf);

        entries.push(ImageEntry {
            path: rel_path,
            source_url: url,
            seed,
            dimensions: [slot.width, slot.height],
            quality_factor: slot.qf,
            size_bytes,
            sha256,
        });

        // Polite delay
        std::thread::sleep(Duration::from_millis(50));
    }

    let elapsed = start.elapsed();
    println!(
        "\nFetched {} images in {:.1}s ({} skipped)",
        entries.len(),
        elapsed.as_secs_f64(),
        skipped.len()
    );

    // Compute distribution counts
    let mut by_qf: HashMap<String, usize> = HashMap::new();
    let mut by_dim: HashMap<String, usize> = HashMap::new();
    for e in &entries {
        *by_qf.entry(e.quality_factor.to_string()).or_insert(0) += 1;
        *by_dim.entry(e.dimensions[0].to_string()).or_insert(0) += 1;
    }

    let total_count = entries.len();
    let manifest = Manifest {
        source: "picsum.photos".to_string(),
        fetched_at: "2026-04-12T22:45:00Z".to_string(),
        total_count,
        by_qf,
        by_dimension: by_dim,
        images: entries,
    };

    let manifest_path = corpus_root.join("manifest.json");
    let manifest_json =
        serde_json::to_string_pretty(&manifest).expect("failed to serialize manifest");
    fs::write(&manifest_path, &manifest_json).expect("failed to write manifest.json");
    println!("Wrote manifest to {}", manifest_path.display());

    // Validation: open every file
    println!("\nValidating all images...");
    let mut valid = 0usize;
    let mut invalid = 0usize;
    for entry in &manifest.images {
        let p = corpus_root.join(&entry.path);
        match image::open(&p) {
            Ok(_) => valid += 1,
            Err(e) => {
                eprintln!("  INVALID: {} — {e}", entry.path);
                invalid += 1;
            }
        }
    }
    println!("  Valid: {valid}, Invalid: {invalid}");

    // Spot-check 5 SHA-256s
    println!("\nSpot-checking 5 SHA-256s...");
    let spots = [
        0,
        total_count / 5,
        total_count * 2 / 5,
        total_count * 3 / 5,
        total_count * 4 / 5,
    ];
    for &i in &spots {
        if i < manifest.images.len() {
            let e = &manifest.images[i];
            let p = corpus_root.join(&e.path);
            let data = fs::read(&p).expect("read failed");
            let actual = sha256_hex(&data);
            let ok = if actual == e.sha256 { "OK" } else { "MISMATCH" };
            println!("  [{ok}] {} {}", e.path, &actual[..16]);
        }
    }

    // Distribution check
    println!("\nDistribution check (target: 22 per bucket):");
    for &qf in qfs {
        for &(label, w, _h) in dims {
            let count = manifest
                .images
                .iter()
                .filter(|e| e.quality_factor == qf && e.dimensions[0] == w)
                .count();
            let status = if (17..=27).contains(&count) {
                "OK"
            } else {
                "WARN"
            };
            println!("  [{status}] qf{qf}/{label}: {count}");
        }
    }

    // Total size
    let total_bytes: u64 = manifest.images.iter().map(|e| e.size_bytes).sum();
    println!(
        "\nTotal corpus size: {:.1} MB",
        total_bytes as f64 / 1_048_576.0
    );

    if !skipped.is_empty() {
        println!("\nSkipped seeds: {:?}", skipped);
    }
}
