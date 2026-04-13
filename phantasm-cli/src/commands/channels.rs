use anyhow::Result;

pub fn run(json: bool) -> Result<()> {
    if json {
        println!("[STUB] JSON output not yet implemented");
    } else {
        println!("[STUB] phantasm channels");
        println!();
        println!("[STUB] Channel Profiles:");
        println!(
            "[STUB] ┌──────────────────┬────┬──────────┬──────────┬────────┬─────────────────┐"
        );
        println!(
            "[STUB] │ Channel          │ QF │ Max Dim  │ Enhance  │ Chroma │ Notes           │"
        );
        println!(
            "[STUB] ├──────────────────┼────┼──────────┼──────────┼────────┼─────────────────┤"
        );
        println!(
            "[STUB] │ lossless         │ —  │ —        │ No       │ —      │ No recompression│"
        );
        println!(
            "[STUB] │ facebook         │ 72 │ 2048px   │ Yes      │ 4:2:0  │ MINICER-style   │"
        );
        println!(
            "[STUB] │ twitter          │ 85 │ 4096px   │ No       │ 4:2:0  │ Varies by size  │"
        );
        println!(
            "[STUB] │ instagram        │ 75 │ 1080px   │ Mild     │ 4:2:0  │ Aggressive      │"
        );
        println!(
            "[STUB] │ whatsapp-photo   │ 60 │ 1600px   │ No       │ 4:2:0  │ Very lossy      │"
        );
        println!(
            "[STUB] │ whatsapp-doc     │ —  │ —        │ No       │ —      │ Document mode   │"
        );
        println!(
            "[STUB] │ signal           │ —  │ —        │ No       │ —      │ Minimal proc.   │"
        );
        println!(
            "[STUB] │ generic-75       │ 75 │ —        │ No       │ 4:2:0  │ Conservative    │"
        );
        println!(
            "[STUB] └──────────────────┴────┴──────────┴──────────┴────────┴─────────────────┘"
        );
    }

    Ok(())
}
