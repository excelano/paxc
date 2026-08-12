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

<file.pax> may be `-`, which reads the source from stdin.

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

/// Read the pax source from a path, or from stdin when the path is `-`.
fn read_source(path: &str) -> std::io::Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        Ok(buf)
    } else {
        fs::read_to_string(path)
    }
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    let mut verbose = false;
    let mut quiet = false;
    let mut debug_only = false;
    let mut color = diagnostic::ColorChoice::default();
    let mut positional: Vec<String> = Vec::new();
    let mut expect_color = false;
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
            "--color" => expect_color = true,
            // A lone `-` is stdin. Anything else leading with `-` is a flag this build
            // does not have, and taking it for a filename hides the mistake.
            other if other.starts_with('-') && other != "-" => {
                eprintln!("paxr: unknown flag '{other}'");
                eprintln!("       see paxr --help");
                process::exit(2);
            }
            _ if expect_color => {
                let Some(c) = diagnostic::ColorChoice::parse(&arg) else {
                    eprintln!("paxr: --color: expected auto, always, or never, not '{arg}'");
                    process::exit(2);
                };
                color = c;
                expect_color = false;
            }
            _ => positional.push(arg),
        }
    }
    if expect_color {
        eprintln!("paxr: --color needs a value: auto, always, or never");
        process::exit(2);
    }
    diagnostic::init_color(color);
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
    let shown = if path == "-" { "stdin" } else { path.as_str() };
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("paxr: cannot read {shown}: {e}");
            process::exit(1);
        }
    };

    let tokens = match lexer::lexer().parse(src.as_str()).into_result() {
        Ok(toks) => toks,
        Err(errs) => {
            for e in &errs {
                diagnostic::from_lex_error(e).report(shown, &src);
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
                diagnostic::from_parse_error(e).report(shown, &src);
            }
            process::exit(1);
        }
    };

    let source_dir = Path::new(path).parent();
    let resolved = match resolver::resolve(&program, source_dir) {
        Ok(r) => r,
        Err(e) => {
            diagnostic::from_resolve_error(&e).report(shown, &src);
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
            diagnostic::from_interpret_error(&e).report(shown, &src);
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
