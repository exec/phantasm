use dct_roundtrip::{round_trip, read_coefficients, compare_snapshots};
use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: dct-roundtrip-spike <input.jpg> <output.jpg>");
        process::exit(1);
    }

    let input = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);

    println!("=== DCT Coefficient Round-Trip Spike ===");
    println!("Input:  {}", input.display());
    println!("Output: {}", output.display());

    // 1. Unmodified round-trip
    println!("\n[1] Unmodified round-trip...");
    match read_coefficients(&input) {
        Ok(snap_a) => {
            match dct_roundtrip::write_coefficients(&input, &output, &snap_a) {
                Ok(()) => {
                    match read_coefficients(&output) {
                        Ok(snap_b) => {
                            match compare_snapshots(&snap_a, &snap_b) {
                                Ok(total) => {
                                    println!("  PASS: {} coefficients match bit-for-bit", total);
                                }
                                Err(e) => {
                                    println!("  FAIL: {}", e);
                                    process::exit(2);
                                }
                            }
                        }
                        Err(e) => { println!("  FAIL reading output: {}", e); process::exit(2); }
                    }
                }
                Err(e) => { println!("  FAIL writing: {}", e); process::exit(2); }
            }
        }
        Err(e) => { println!("  FAIL reading input: {}", e); process::exit(2); }
    }

    // 2. Modified round-trip
    println!("\n[2] Modified round-trip (one coefficient ±1)...");
    match round_trip(&input, &output, true) {
        Ok(result) => {
            println!(
                "  Modified comp={} block_row={} block_col={} coef_idx={}",
                result.modified_comp.unwrap(),
                result.modified_block_row.unwrap(),
                result.modified_block_col.unwrap(),
                result.modified_coef_idx.unwrap(),
            );
            println!(
                "  original={} -> written={} -> readback={}",
                result.original_value.unwrap(),
                result.new_value.unwrap(),
                result.verified_value.unwrap(),
            );
            let wrote = result.new_value.unwrap();
            let read = result.verified_value.unwrap();
            if wrote == read {
                println!("  PASS: modification persisted exactly, {} total coefficients verified", result.total_coeffs);
            } else {
                println!("  FAIL: wrote {} but read back {}", wrote, read);
                process::exit(2);
            }
        }
        Err(e) => {
            println!("  FAIL: {}", e);
            process::exit(2);
        }
    }

    println!("\n=== All checks passed ===");
}
