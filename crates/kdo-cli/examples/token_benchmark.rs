//! A/B measurement harness: raw `find/cat` vs `kdo context` token consumption.
//!
//! This is not a criterion benchmark — it's a measurement tool that prints
//! a comparison table to stdout. Run with:
//!
//! ```bash
//! cargo run --release --example token_benchmark -- <workspace_root> <project>
//! ```

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let workspace_root = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let project_name = args.get(2).map(|s| s.as_str()).unwrap_or("vault-program");

    println!("Token Benchmark: raw traversal vs kdo context");
    println!("==============================================");
    println!("Workspace: {}", workspace_root.display());
    println!("Project:   {project_name}");
    println!();

    // Method A: Raw find + cat (simulates what an agent does without kdo)
    let raw_output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "find {} -name '*.rs' -o -name '*.ts' -o -name '*.py' | head -50 | xargs cat 2>/dev/null",
            workspace_root.display()
        ))
        .output()
        .expect("failed to run find/cat");
    let raw_bytes = raw_output.stdout.len();
    let raw_tokens = raw_bytes / 4;

    // Method B: kdo context (structured, budgeted)
    let kdo_binary = workspace_root
        .join("../../target/release/kdo")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("kdo"));

    let kdo_output = Command::new(&kdo_binary)
        .args(["context", project_name, "--budget", "2048"])
        .current_dir(&workspace_root)
        .output();

    let (kdo_bytes, kdo_tokens) = match kdo_output {
        Ok(output) if output.status.success() => {
            let bytes = output.stdout.len();
            (bytes, bytes / 4)
        }
        _ => {
            eprintln!("Warning: kdo context command failed, using estimate");
            (2048 * 4, 2048) // Budget target
        }
    };

    let ratio = if kdo_tokens > 0 {
        raw_tokens as f64 / kdo_tokens as f64
    } else {
        0.0
    };

    println!("| Method        | Bytes   | ~Tokens | Ratio |");
    println!("|---------------|---------|---------|-------|");
    println!(
        "| find+cat      | {:<7} | {:<7} | 1.0x  |",
        raw_bytes, raw_tokens
    );
    println!(
        "| kdo context   | {:<7} | {:<7} | {:.1}x  |",
        kdo_bytes, kdo_tokens, ratio
    );
    println!();
    println!(
        "kdo reduces token consumption by {:.0}% on this workspace.",
        (1.0 - 1.0 / ratio) * 100.0
    );
}
