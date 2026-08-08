//! paxr — the pax runner (interpreter).
//!
//! Reads a .pax file, parses and resolves it the same way paxc does, and
//! then executes it in-process so the developer can exercise their logic
//! without going through Power Automate. Lives alongside paxc in the same
//! crate, sharing the lexer / parser / resolver via the library.

use chumsky::prelude::*;
use paxc::{cli, diagnostic, interpreter, lexer, parser, resolver, skill};
use std::path::Path;
use std::{env, fs, process};

/// paxr's own flags. The shared tail is appended at use.
const USAGE_HEAD: &str = "\
usage: paxr [--verbose | --quiet | --debug] <file.pax>

Runs a .pax file in-process so you can exercise the logic without going
through Power Automate. Prints the end state of every variable unless
told otherwise. Connector actions (`pa <Name>`) are skipped -- paxr can't
call a real connector -- and reported as it goes.

Output modes (mutually exclusive):
  --verbose, -v  trace each action as it executes
  --quiet, -q    print nothing but errors
  --debug, -d    print only debug() output
";

fn usage_text() -> String {
    format!("{USAGE_HEAD}\n{}", cli::COMMON_FLAGS)
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    let mut verbose = false;
    let mut quiet = false;
    let mut debug_only = false;
    let mut positional: Vec<String> = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => cli::help(&usage_text()),
            "--version" | "-V" => {
                println!("paxr {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            // The skill covers both binaries, so either can install it; the
            // idempotence check makes them interchangeable.
            "--install-skill" => process::exit(skill::install()),
            "--uninstall-skill" => process::exit(skill::uninstall()),
            "--verbose" | "-v" => verbose = true,
            "--quiet" | "-q" => quiet = true,
            "--debug" | "-d" => debug_only = true,
            _ => positional.push(arg),
        }
    }
    // --verbose, --quiet, --debug are pairwise mutually exclusive.
    let mode_count = [verbose, quiet, debug_only].iter().filter(|b| **b).count();
    if mode_count > 1 {
        eprintln!("paxr: --verbose, --quiet, and --debug are mutually exclusive");
        process::exit(2);
    }
    if positional.len() != 1 {
        cli::usage(&usage_text());
    }
    let path = &positional[0];
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("paxr: cannot read {path}: {e}");
            process::exit(1);
        }
    };

    let tokens = match lexer::lexer().parse(src.as_str()).into_result() {
        Ok(toks) => toks,
        Err(errs) => {
            for e in &errs {
                diagnostic::from_lex_error(e).report(path, &src);
            }
            process::exit(1);
        }
    };

    let program = match parser::parser()
        .parse(
            tokens
                .as_slice()
                .map((src.len()..src.len()).into(), |(t, s)| (t, s)),
        )
        .into_result()
    {
        Ok(p) => p,
        Err(errs) => {
            for e in &errs {
                diagnostic::from_parse_error(e).report(path, &src);
            }
            process::exit(1);
        }
    };

    let source_dir = Path::new(path).parent();
    let resolved = match resolver::resolve(&program, source_dir) {
        Ok(r) => r,
        Err(e) => {
            diagnostic::from_resolve_error(&e).report(path, &src);
            process::exit(1);
        }
    };

    let config = interpreter::Config {
        verbose,
        quiet,
        debug_only,
    };
    let state = match interpreter::interpret_with(&src, &resolved, config) {
        Ok(s) => s,
        Err(e) => {
            diagnostic::from_interpret_error(&e).report(path, &src);
            process::exit(1);
        }
    };

    if !quiet && !debug_only {
        let dump = interpreter::format_state_dump(&state);
        if !dump.is_empty() {
            println!();
            println!("end state:");
            print!("{}", dump);
        }
    }
}
