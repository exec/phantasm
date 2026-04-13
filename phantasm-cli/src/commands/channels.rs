use anyhow::Result;
use phantasm_core::{ChannelProfile, ChromaSub};

pub fn run(json: bool) -> Result<()> {
    let profiles: Vec<ChannelProfile> = ChannelProfile::all_builtin_names()
        .iter()
        .filter_map(|name| ChannelProfile::builtin(name))
        .collect();

    if json {
        print_json(&profiles);
    } else {
        print_table(&profiles);
    }

    Ok(())
}

fn print_table(profiles: &[ChannelProfile]) {
    println!("Channel Profiles:");
    println!("┌──────────────────┬────┬──────────┬──────────┬────────┬──────────────────┐");
    println!("│ Channel          │ QF │ Max Dim  │ Enhance  │ Chroma │ Notes            │");
    println!("├──────────────────┼────┼──────────┼──────────┼────────┼──────────────────┤");
    for p in profiles {
        println!(
            "│ {:<16} │ {:<2} │ {:<8} │ {:<8} │ {:<6} │ {:<16} │",
            p.name,
            qf_cell(p),
            max_dim_cell(p),
            enhance_cell(p),
            chroma_cell(p),
            notes_cell(&p.name),
        );
    }
    println!("└──────────────────┴────┴──────────┴──────────┴────────┴──────────────────┘");
}

fn print_json(profiles: &[ChannelProfile]) {
    println!("[");
    let last = profiles.len().saturating_sub(1);
    for (i, p) in profiles.iter().enumerate() {
        println!("  {{");
        println!("    \"name\": \"{}\",", p.name);
        println!("    \"jpeg_quality\": {},", json_opt_u8(p.jpeg_quality));
        println!("    \"max_dimension\": {},", json_opt_u32(p.max_dimension));
        println!(
            "    \"chroma_subsampling\": \"{}\",",
            chroma_json(p.chroma_subsampling)
        );
        println!("    \"applies_enhancement\": {},", p.applies_enhancement);
        println!("    \"strips_metadata\": {},", p.strips_metadata);
        println!(
            "    \"overflow_strategy\": \"{}\"",
            overflow_str(p.overflow_strategy)
        );
        if i == last {
            println!("  }}");
        } else {
            println!("  }},");
        }
    }
    println!("]");
}

fn qf_cell(p: &ChannelProfile) -> String {
    match p.jpeg_quality {
        Some(q) => q.to_string(),
        None => "—".to_string(),
    }
}

fn max_dim_cell(p: &ChannelProfile) -> String {
    match p.max_dimension {
        Some(d) => format!("{}px", d),
        None => "—".to_string(),
    }
}

fn enhance_cell(p: &ChannelProfile) -> &'static str {
    if p.applies_enhancement {
        "Yes"
    } else {
        "No"
    }
}

fn chroma_cell(p: &ChannelProfile) -> &'static str {
    chroma_str(p.chroma_subsampling)
}

fn chroma_str(c: ChromaSub) -> &'static str {
    match c {
        ChromaSub::C444 => "4:4:4",
        ChromaSub::C422 => "4:2:2",
        ChromaSub::C420 => "4:2:0",
        ChromaSub::None => "—",
    }
}

fn chroma_json(c: ChromaSub) -> &'static str {
    match c {
        ChromaSub::C444 => "4:4:4",
        ChromaSub::C422 => "4:2:2",
        ChromaSub::C420 => "4:2:0",
        ChromaSub::None => "none",
    }
}

fn overflow_str(o: phantasm_core::OverflowStrategy) -> &'static str {
    use phantasm_core::OverflowStrategy::*;
    match o {
        None => "none",
        Clamp => "clamp",
        BoundaryOnly => "boundary_only",
        Full => "full",
    }
}

fn notes_cell(name: &str) -> &'static str {
    match name {
        "lossless" => "No recompression",
        "facebook" => "MINICER-style",
        "twitter" => "Varies by size",
        "instagram" => "Aggressive",
        "whatsapp-photo" => "Very lossy",
        "whatsapp-doc" => "Document mode",
        "signal" => "Minimal proc.",
        "generic-75" => "Conservative",
        _ => "",
    }
}

fn json_opt_u8(v: Option<u8>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "null".to_string(),
    }
}

fn json_opt_u32(v: Option<u32>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eight_builtins_resolve() {
        let names = ChannelProfile::all_builtin_names();
        assert_eq!(names.len(), 8);
        for n in names {
            assert!(ChannelProfile::builtin(n).is_some(), "missing builtin: {n}");
        }
    }

    #[test]
    fn notes_defined_for_all_builtins() {
        for n in ChannelProfile::all_builtin_names() {
            assert!(!notes_cell(n).is_empty(), "missing note for {n}");
        }
    }

    #[test]
    fn run_table_ok() {
        run(false).unwrap();
    }

    #[test]
    fn run_json_ok() {
        run(true).unwrap();
    }
}
