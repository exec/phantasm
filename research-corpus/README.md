# research-corpus

JPEG image corpus for benchmarking content-adaptive distortion functions in the phantasm steganography research phase.

## Purpose

Provides ~198 varied photographic JPEGs with controlled quality factors and dimensions. Used by the eval harness to measure steganalysis detection error as a function of QF and image resolution.

## Source and License

Images fetched from [picsum.photos](https://picsum.photos), which serves a curated subset of Unsplash photos. License: Unsplash License (effectively CC0 for research/non-commercial use). No API key required.

## Distribution

| QF  | 512×512 | 1024×1024 | 720×680 | Total |
|-----|---------|-----------|---------|-------|
| 75  | 22      | 22        | 22      | 66    |
| 85  | 22      | 22        | 22      | 66    |
| 90  | 22      | 22        | 22      | 66    |
| **Total** | **66** | **66** | **66** | **198** |

Total corpus size: ~22.1 MB

## Directory structure

```
research-corpus/
├── qf75/512/   — 22 JPEGs, 512×512, QF=75
├── qf75/1024/  — 22 JPEGs, 1024×1024, QF=75
├── qf75/720/   — 22 JPEGs, 720×680, QF=75
├── qf85/…      — (same structure, QF=85)
├── qf90/…      — (same structure, QF=90)
├── manifest.json
└── README.md
```

Files are named `0001.jpg` through `0022.jpg` within each bucket.

## How to regenerate

```sh
cargo run --release -p phantasm-image --example fetch_corpus
```

The script fetches each image from `https://picsum.photos/seed/<seed>/<width>/<height>`, then re-encodes at the target QF using `image::codecs::jpeg::JpegEncoder::new_with_quality`. Seeds are `phantasm-0001` through `phantasm-0198`.

Source: `phantasm-image/examples/fetch_corpus.rs`

## Known limitations

- **Same underlying Unsplash set as Spike B's corpus.** Picsum uses a fixed pool of Unsplash images; different seeds select different crops from that pool.
- **Picsum resamples from originals.** Absolute pixel content may vary slightly between requests with the same seed (e.g., if their CDN re-encodes). The SHA-256 values in `manifest.json` reflect the state at fetch time.
- **Re-compression.** Images are decoded and re-saved at the target QF, so they are not original-quality originals. This is intentional — it ensures consistent quantization tables across the corpus.
- **Image files are gitignored.** They are regenerable from the seeds in `manifest.json`. Run the script above to restore them.
