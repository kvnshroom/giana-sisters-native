use giana_extract::image::{diff_ranges, PATCHES};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: giana-extract <file.g64|file.d64> <out_dir> [--compare <reference ingame.bin>]
       giana-extract --check <data_dir>

Writes every data file of tools/data/manifest.txt into <out_dir> (program image with the documented patches, the
derived tables, the title picture) and verifies each against the expected SHA-256. --check only verifies a directory.";

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = Vec::new();
    let mut compare = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--compare" => compare = Some(it.next().ok_or("--compare needs a file")?.clone()),
            "--check" => {
                let dir = it.next().ok_or("--check needs a directory")?;
                let problems = giana_extract::verify_dir(Path::new(dir));
                for p in &problems {
                    eprintln!("{p}");
                }
                return if problems.is_empty() { println!("{dir}: all {} files ok", giana_extract::MANIFEST.len()); Ok(()) } else { Err("data directory is not valid".into()) };
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => positional.push(a.clone()),
        }
    }
    let [input, out_dir] = positional.as_slice() else {
        return Err(USAGE.into());
    };
    let image = std::fs::read(input).map_err(|e| format!("cannot read {input}: {e}"))?;
    let ex = giana_extract::extract(&image)?;
    println!("input: {input}\nversion: {}", ex.version.name);
    let files = giana_extract::derive::derive_all_with(ex.patched.clone(), ex.title_prg.clone(), ex.atg.clone())?;
    giana_extract::write_data_dir(&files, Path::new(out_dir))?;
    for f in &files {
        println!("wrote {}/{} ({} bytes, sha256 ok)", out_dir, f.name, f.data.len());
    }
    println!("patches applied to ingame.bin:");
    for p in PATCHES {
        println!("  ${:04X} ({} byte{}): {}", p.addr, p.bytes.len(), if p.bytes.len() == 1 { "" } else { "s" }, p.why);
    }

    if let Some(refpath) = compare {
        let reference = std::fs::read(&refpath).map_err(|e| format!("cannot read {refpath}: {e}"))?;
        if reference.len() != ex.patched.len() {
            return Err(format!("reference has {} bytes, expected {}", reference.len(), ex.patched.len()));
        }
        let d = diff_ranges(&ex.patched, &reference);
        let total: usize = d.iter().map(|r| r.1).sum();
        let program_diffs: usize = d
            .iter()
            .filter(|(s, _)| (0x0820..=0xFFF0).contains(s))
            .map(|r| r.1)
            .sum();
        println!(
            "compare with {refpath}: {total} differing bytes in {} ranges; inside the program area $0820..$FFF0: {program_diffs}",
            d.len()
        );
        if !d.is_empty() {
            println!("  (outside the program area the reference holds a RAM snapshot, the extractor writes zeros)");
            for (s, l) in d.iter().take(8) {
                println!("  ${s:04X} +{l}");
            }
            if d.len() > 8 {
                println!("  ... {} more ranges", d.len() - 8);
            }
        }
        if program_diffs != 0 {
            return Err("the program area differs from the reference".into());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("giana-extract: {e}");
            ExitCode::FAILURE
        }
    }
}
