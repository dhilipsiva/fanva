//! fanva-validate — batch Lojban validation via stdin.
//!
//! Reads one sentence per line from stdin and runs the local gates
//! (`gerna::parse_checked` → `smuni::compile_from_gerna_ast`) on each.
//! Outputs one JSON object per line to stdout:
//!   {"line":"...","valid":true}
//!   {"line":"...","valid":false,"error":"[Syntax Error] ..."}
//!
//! This is the validator interface of the python Lojban flywheel
//! (python/generate_training_data.py and python/nibli_model.py subprocess it).
//! Ported from nibli's `nibli-validate`; fanva is Lojban-only, so `--lang
//! lojban` is accepted for compatibility and anything else is a hard error.

use std::io::{self, BufRead};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lang" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("error: --lang needs a value (lojban)");
                    return ExitCode::FAILURE;
                };
                if value != "lojban" {
                    eprintln!("error: fanva-validate is Lojban-only (got --lang {value})");
                    return ExitCode::FAILURE;
                }
            }
            other => {
                eprintln!(
                    "error: unexpected argument '{other}' (usage: fanva-validate [--lang lojban])"
                );
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let escaped_line = escape_json(trimmed);
        match fanva::gates::local_gates(trimmed) {
            Ok(_) => println!(r#"{{"line":"{}","valid":true}}"#, escaped_line),
            Err(e) => {
                let escaped_err = escape_json(e.message());
                println!(
                    r#"{{"line":"{}","valid":false,"error":"{}"}}"#,
                    escaped_line, escaped_err
                );
            }
        }
    }

    ExitCode::SUCCESS
}

/// Escape a string for embedding in JSON.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r#"\\"#),
            '\n' => out.push_str(r#"\n"#),
            '\r' => out.push_str(r#"\r"#),
            '\t' => out.push_str(r#"\t"#),
            c => out.push(c),
        }
    }
    out
}
