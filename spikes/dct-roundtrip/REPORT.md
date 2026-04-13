# DCT FFI Round-Trip Spike Report

## FFI Approach Chosen

**`mozjpeg-sys` v2.2.4-beta.1**

Rationale: vendors libjpeg-turbo statically via a build script (no system library dependency), exposes the full libjpeg C API including `jpeg_read_coefficients`, `jpeg_write_coefficients`, `jpeg_copy_critical_parameters`, and the virtual block array accessor `access_virt_barray`. Builds cleanly on macOS ARM64 out of the box. Direct `bindgen` against system libjpeg-turbo was rejected because it requires system library installation and version management.

## libjpeg-turbo Gotchas

1. **`mem` field is on `common`, not the decompress struct directly.** Correct path: `cinfo.common.mem`, not `cinfo.mem`.

2. **`access_virt_barray` requires `writable=true` for write access.** Passing `false` silently discards modifications.

3. **`jpeg_copy_critical_parameters` copies quantization and Huffman tables** — essential for bit-exact round-trip. The output JPEG must use the same tables as the source.

4. **Mid-frequency AC coefficients are often zero** in synthetic test images. The spike handles this by applying +1 unconditionally, which is correct for a spike (zero-value coefficients are valid to modify).

## Proof of Bit-Exact Round-Trip

```
running 4 tests
unmodified roundtrip: 786432 coefficients matched
test test_unmodified_roundtrip_512 ... ok
single coeff modification: orig=0 new=1 readback=1, 786432 total coefficients verified
test test_single_coeff_modification ... ok
1024 fixture single coeff: orig=0 -> written=1 -> readback=1, 3145728 coefficients verified
test test_single_coeff_modification_1024 ... ok
1024x1024 round-trip: 3145728 total coefficients verified bit-exact
test test_roundtrip_1024 ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
```

Round-trip is **bit-exact**. 786,432 coefficients for 512×512; 3,145,728 for 1024×1024.

## Recommendation for Phase 0.1 (`phantasm-image`)

1. Use `mozjpeg-sys` directly — the higher-level `mozjpeg` crate abstracts away the coefficient API.
2. Wrap unsafe FFI in a safe `JpegCoefficients` struct with indexed access. The spike's `CoeffSnapshot` is a reasonable model.
3. Copy JPEG markers (EXIF, ICC) via `jpeg_save_markers` + `jpeg_write_marker` — the spike omits this.
4. The write path requires the source file to remain open throughout; design the API so source and dest lifetimes are managed together.
5. Coefficient modification must respect quantization step size for steganographic purposes — at Q85+, most AC steps are 1; at lower quality, larger deltas may be needed to survive re-encode. A Phase 0.1 design decision, not a blocker.
