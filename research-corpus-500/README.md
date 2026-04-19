# research-corpus-500

500-image single-bucket cover corpus for the **cover-source diversity validation** follow-up to ML_STEGANALYSIS.md Update 3.

## Purpose

Update 3 established that phantasm J-UNIWARD is detected at 89.9% by a CNN fine-tuned on 198 covers × 5 passphrases. The open question is whether that number is a **cover-pool artifact** — too few unique covers (22 seeds) so the detector overfits the specific image content — or a robust property of phantasm's modification pattern that would hold on a much larger cover set. This corpus isolates the cover-diversity variable by holding qf/dim fixed while expanding the unique cover count ~2.5×.

## Distribution

| QF | 720×680 | Total |
|----|--------:|------:|
| 85 | 500     | 500   |

Single qf/dim bucket chosen for clean statistics. `qf85/720` matches the modal ML-eval crop size used in Updates 1-6 and an audit-favored quality tier.

## Directory structure

```
research-corpus-500/
├── qf85/720/     — 500 JPEGs, 720×680, QF=85 (gitignored)
├── .gitignore
├── manifest.json
└── README.md
```

Files are named `0001.jpg` through `0500.jpg`, matching seed numbers `phantasm-0001` through `phantasm-0500`.

## How to regenerate

```sh
MODE=diversity500 cargo run --release -p phantasm-image --example fetch_corpus
```

The script fetches each image from `https://picsum.photos/seed/phantasm-NNNN/720/680`, then re-encodes at QF=85 via `image::codecs::jpeg::JpegEncoder::new_with_quality`.

Source: `phantasm-image/examples/fetch_corpus.rs` (dispatches on the `MODE` env var).

## Relationship to `research-corpus/`

- `research-corpus/` (the original 198-image matrix): seeds phantasm-0001..0198 across 3 QF × 3 dim.
- `research-corpus-500/` (this corpus): seeds phantasm-0001..0500 at a single qf/dim.

The seed overlap with the first 198 is **intentional** — the same seed produces the same Picsum URL, but the re-encoding parameters differ (different qf/dim buckets), so the JPEGs are distinct. The 302 seeds `phantasm-0199..phantasm-0500` are the **new** unique covers.

## Known limitations

Same as `research-corpus/` README: same Unsplash pool, Picsum may re-encode across requests, and we decode+re-encode at the target QF so these are not original-quality originals. The SHA-256s in `manifest.json` reflect the state at fetch time.
