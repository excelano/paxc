use chumsky::prelude::*;
use paxc::pa::{decoder, emitter, packager};
use paxc::{check, cli, diagnostic, lexer, parser, resolver, skill};
use std::path::{Path, PathBuf};
use std::{env, fs, process};

struct Args {
    path: String,
    target: Option<packager::Target>,
    name: Option<String>,
    out: Option<PathBuf>,
    decode: bool,
    check: bool,
    out_dir: Option<PathBuf>,
    /// Finding codes demoted from error to warning. Still reported, still
    /// visible — just not fatal.
    allow: Vec<String>,
    color: diagnostic::ColorChoice,
}

/// paxc's own flags. The shared tail is appended at use.
const USAGE_HEAD: &str = "\
usage: paxc [--target <pa-legacy>] [--name <NAME>] [--out <PATH>] [--allow <CODE>] <file.pax>

<file.pax> may be `-`, which reads the source from stdin. The pa/ folder is then
looked for in the current directory, since there is no source file to sit beside.

With no --target: writes the Power Automate flow definition JSON to stdout.
With --target pa-legacy: writes a legacy PA import package (.zip). Defaults:
  --name  input file basename without .pax (or pa/flow.json's displayName when present)
  --out   <name>.zip in the current directory

Compiling also checks the pa/ bodies it carries verbatim, and reports what it
finds against the file you edited. Anything reported as an error fails the
compile and nothing is written.

  --list-codes     print every finding code, one per line, and exit
  --allow <CODE>   demote one finding code from error to warning; repeatable.
                   Still reported, just not fatal -- for when paxc is wrong
                   about your flow and you would rather ship than wait. Applies
                   to --check too. An unknown code is rejected rather than
                   ignored.

Decode mode (round-trip ingest):
  paxc --decode <flow.json|flow.zip> [--out-dir <DIR>]
Reads an exported PA flow definition (either the inner definition.json
or a legacy import package .zip) and writes a .pax source file plus a
pa/ folder of opaque action bodies to <DIR>. For a .json input, --out-dir
defaults to the input's parent directory; for a .zip, it defaults to a
sister directory named after the zip's stem.

Check mode (validate an exported flow):
  paxc --check <flow.json|flow.zip>
Reads a flow definition and reports problems in it without compiling or
decoding. Works on any exported flow, including ones never written in pax.
Exits 1 if anything is reported as an error.
";

fn usage_text() -> String {
    format!("{USAGE_HEAD}\n{}", cli::COMMON_FLAGS)
}

/// Print usage to stderr and exit 2 — the error path for a malformed invocation.
fn usage() -> ! {
    cli::usage(&usage_text())
}

/// Print usage to stdout and exit 0 — the success path for an explicit --help.
fn help() -> ! {
    cli::help(&usage_text())
}

fn parse_args() -> Args {
    let argv: Vec<String> = env::args().skip(1).collect();
    let mut target: Option<packager::Target> = None;
    let mut name: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut decode = false;
    let mut check = false;
    let mut out_dir: Option<PathBuf> = None;
    let mut allow: Vec<String> = Vec::new();
    let mut color = diagnostic::ColorChoice::default();
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--help" | "-h" => help(),
            "--version" | "-V" => {
                println!("paxc {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            // Terminal actions: they touch the user's skills directory and
            // nothing else, so no source file is read on the way through.
            "--install-skill" => process::exit(skill::install()),
            "--uninstall-skill" => process::exit(skill::uninstall()),
            "--target" => {
                i += 1;
                let Some(v) = argv.get(i) else { usage() };
                target = Some(match v.as_str() {
                    "pa-legacy" => packager::Target::PaLegacy,
                    other => {
                        eprintln!("paxc: unknown target '{other}' (supported: pa-legacy)");
                        process::exit(2);
                    }
                });
            }
            "--name" => {
                i += 1;
                let Some(v) = argv.get(i) else { usage() };
                name = Some(v.clone());
            }
            "--out" => {
                i += 1;
                let Some(v) = argv.get(i) else { usage() };
                out = Some(PathBuf::from(v));
            }
            "--decode" => {
                decode = true;
            }
            "--check" => {
                check = true;
            }
            "--out-dir" => {
                i += 1;
                let Some(v) = argv.get(i) else { usage() };
                out_dir = Some(PathBuf::from(v));
            }
            "--allow" => {
                i += 1;
                let Some(v) = argv.get(i) else { usage() };
                // Rejecting an unknown code rather than ignoring it: a
                // misspelled `--allow` that quietly allows nothing would fail
                // the build for the reason it was passed to prevent, and the
                // user would be looking at the finding rather than the flag.
                if !check::ALL_CODES.contains(&v.as_str()) {
                    eprintln!("paxc: --allow: no finding code named '{v}'");
                    if let Some(near) = nearest_code(v) {
                        eprintln!("       did you mean '{near}'?");
                    }
                    eprintln!("       codes: {}", check::ALL_CODES.join(", "));
                    process::exit(2);
                }
                allow.push(v.clone());
            }
            "--list-codes" => {
                for code in check::ALL_CODES {
                    println!("{code}");
                }
                process::exit(0);
            }
            "--color" => {
                i += 1;
                let Some(v) = argv.get(i) else { usage() };
                let Some(c) = diagnostic::ColorChoice::parse(v) else {
                    eprintln!("paxc: --color: expected auto, always, or never, not '{v}'");
                    process::exit(2);
                };
                color = c;
            }
            // A lone `-` is stdin, a positional. Anything else that leads with `-` is a
            // flag this build does not have: rejecting it beats the old behaviour of
            // taking it for a filename, which reported a missing file and sent the
            // caller looking at the disk instead of at the command.
            other if other.starts_with('-') && other != "-" => {
                eprintln!("paxc: unknown flag '{other}'");
                if let Some(near) = nearest_flag(other) {
                    eprintln!("       did you mean '{near}'?");
                }
                eprintln!("       see paxc --help");
                process::exit(2);
            }
            _ => positional.push(argv[i].clone()),
        }
        i += 1;
    }

    if positional.len() != 1 {
        usage();
    }
    Args {
        path: positional.into_iter().next().unwrap(),
        target,
        name,
        out,
        decode,
        check,
        out_dir,
        allow,
        color,
    }
}

/// Read the pax source from a path, or from stdin when the path is `-`.
///
/// `-` is the Unix spelling of "this argument is stdin"; without it a caller who writes it
/// gets a missing-file error naming a file they never meant to open.
fn read_source(path: &str) -> std::io::Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        Ok(buf)
    } else {
        fs::read_to_string(path)
    }
}

/// How to name the input in a message: `-` is stdin, and saying so beats printing a dash.
fn display_path(path: &str) -> &str {
    if path == "-" { "stdin" } else { path }
}

/// Every flag paxc accepts, for the unknown-flag hint.
const ALL_FLAGS: [&str; 13] = [
    "--help",
    "--version",
    "--install-skill",
    "--uninstall-skill",
    "--target",
    "--name",
    "--out",
    "--decode",
    "--check",
    "--out-dir",
    "--allow",
    "--list-codes",
    "--color",
];

/// Closest known flag to a misspelling. Reuses the edit-distance helper behind the
/// `--allow` hint, so a mistyped flag gets the same treatment as a mistyped code.
fn nearest_flag(given: &str) -> Option<&'static str> {
    let bare = given.trim_start_matches('-');
    ALL_FLAGS
        .iter()
        .find(|f| check::expressions::close(bare, f.trim_start_matches('-')))
        .copied()
}

/// Closest finding code to a misspelling, for the `--allow` error. Reuses the
/// same edit-distance helper the expression hints use, so a near miss gets the
/// same treatment as a mistyped function name.
fn nearest_code(given: &str) -> Option<&'static str> {
    check::ALL_CODES
        .iter()
        .find(|c| check::expressions::close(given, c))
        .copied()
}

fn main() {
    let args = parse_args();
    diagnostic::init_color(args.color);

    // --decode and --check take a .json or .zip and probe it by extension and by seeking
    // inside the archive, neither of which a pipe supports.
    if args.path == "-" && (args.decode || args.check) {
        let mode = if args.decode { "--decode" } else { "--check" };
        eprintln!(
            "paxc: {mode} reads an exported flow from a file, not stdin — give the .json or .zip path"
        );
        process::exit(2);
    }

    if args.check {
        run_check(&args);
        return;
    }

    if args.decode {
        run_decode(&args);
        return;
    }

    let src = match read_source(&args.path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("paxc: cannot read {}: {}", display_path(&args.path), e);
            process::exit(1);
        }
    };

    let tokens = match lexer::lexer().parse(src.as_str()).into_result() {
        Ok(toks) => toks,
        Err(errs) => {
            for e in &errs {
                diagnostic::from_lex_error(e).report(display_path(&args.path), &src);
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
                diagnostic::from_parse_error(e).report(display_path(&args.path), &src);
            }
            process::exit(1);
        }
    };

    // `pa/` sits beside the source file. Read from stdin there is no such file, so the
    // caller's own directory is the only sensible place to look.
    let source_dir = if args.path == "-" {
        Some(Path::new("."))
    } else {
        Path::new(&args.path).parent()
    };
    let resolved = match resolver::resolve(&program, source_dir) {
        Ok(r) => r,
        Err(e) => {
            diagnostic::from_resolve_error(&e).report(display_path(&args.path), &src);
            process::exit(1);
        }
    };

    // Before anything is written. A compiler that reports a flow as broken and
    // then hands over the .zip anyway has told the user their error is
    // optional.
    if report_pa_body_findings(&resolved, source_dir, &args.allow) {
        process::exit(1);
    }

    match args.target {
        None => {
            let json = emitter::emit(&resolved);
            println!("{}", serde_json::to_string_pretty(&json).unwrap());

            let dropped = emitter::count_debug_actions(&resolved.actions);
            if dropped > 0 {
                let plural = if dropped == 1 { "" } else { "s" };
                eprintln!("note: dropped {dropped} debug() statement{plural}");
            }
        }
        Some(target) => {
            // Default --name: pa/flow.json's displayName when present, else
            // the input file basename. Lets a decoded flow round-trip back to
            // its original PA displayName without re-typing it on encode.
            let derived_name = args.name.unwrap_or_else(|| {
                read_display_name(source_dir).unwrap_or_else(|| derive_name_from_path(&args.path))
            });
            let out_path = args
                .out
                .unwrap_or_else(|| PathBuf::from(format!("{derived_name}.zip")));
            if let Err(e) = packager::package(&resolved, target, &derived_name, &out_path) {
                eprintln!("paxc: packaging failed: {e}");
                process::exit(1);
            }
            eprintln!("wrote {}", out_path.display());

            let dropped = emitter::count_debug_actions(&resolved.actions);
            if dropped > 0 {
                let plural = if dropped == 1 { "" } else { "s" };
                eprintln!("note: dropped {dropped} debug() statement{plural}");
            }
        }
    }
}

/// Run the flow checks over what this compile is about to produce, and report
/// whatever landed inside a `pa/` body.
///
/// paxc validates pax source and, until now, took `pa/` bodies entirely on
/// trust: a reference to an action that does not exist and a misspelled PA
/// function both compiled clean and failed at run time in the tenant. The
/// checks could already see them, but only if the user thought to run `--check`
/// against the output afterwards and then map a path in generated JSON back to
/// the file they had edited.
///
/// Rendered through the same ariadne path as a pax compile error, underlining
/// the line in the `pa/` file rather than naming the file and leaving the
/// reader to search a connector body several hundred lines deep. When the field
/// cannot be located in the text — PA defaults some it never wrote down — the
/// report falls back to naming the file, which is still better than pointing at
/// an arbitrary line.
///
/// Returns whether anything fatal was found, so the caller can stop before
/// writing a flow it has just said is broken.
///
/// A code named in `--allow` still gets reported, in full, and simply is not
/// fatal. Silencing it would throw away the signal that made per-code narrower
/// than a blanket switch: the point is to unblock somebody without also making
/// the finding invisible to them.
fn report_pa_body_findings(
    resolved: &resolver::ResolvedProgram,
    source_dir: Option<&Path>,
    allow: &[String],
) -> bool {
    let sources = emitter::pa_source_map(&resolved.actions);
    let Ok(findings) = check::check_flow(&emitter::emit(resolved)) else {
        // The shape came straight from the emitter, so it is checkable by
        // construction. If that ever stops being true it is a paxc bug, and
        // failing a compile over it would be the wrong response.
        return false;
    };
    let split = check::attribute_to_sources(findings, &sources, source_dir);
    let mut fatal = false;

    for attributed in &split.in_pa_bodies {
        let finding = &attributed.finding;
        let allowed = allow.iter().any(|a| a == finding.code);
        let severity = if allowed {
            check::Severity::Warning
        } else {
            finding.severity
        };
        fatal |= severity == check::Severity::Error;

        let message = format!("[{}] {}", finding.code, finding.message);

        // Re-reading rather than keeping the bytes from resolve: the file is
        // small, this runs once per finding on a path that is already writing
        // to a terminal, and a body that vanished mid-compile should degrade to
        // a plain line rather than take the compile with it.
        let Ok(text) = fs::read_to_string(&attributed.source) else {
            eprintln!("{finding}");
            continue;
        };

        let mut diagnostic = match check::locate::locate(&text, &attributed.pointer) {
            Some(range) => diagnostic::Diagnostic::at_range(message, range, "here"),
            None => diagnostic::Diagnostic::unspanned(message),
        };
        if severity == check::Severity::Warning {
            diagnostic = diagnostic.as_warning();
        }
        if let Some(note) = &finding.note {
            diagnostic = diagnostic.with_note(note);
        }
        if allowed {
            diagnostic = diagnostic.with_note(format!("allowed by --allow {}", finding.code));
        }
        diagnostic.report(&attributed.display, &text);
    }

    // Findings against JSON paxc wrote itself. The author cannot fix these and
    // did not cause them, but shipping a flow paxc knows is broken is the worst
    // available outcome, so they are fatal and they say whose fault it is.
    // `--allow` deliberately does not reach them: silencing our own bug is not
    // a thing a user should be able to ask for by accident.
    for finding in &split.in_generated_output {
        fatal = true;
        eprintln!(
            "paxc: internal: emitted a flow that fails paxc's own checks at {}",
            finding.path
        );
        eprintln!("      [{}] {}", finding.code, finding.message);
        eprintln!("      this is a paxc bug, not a problem with your source.");
        eprintln!("      please report it at https://github.com/excelano/paxc/issues");
    }

    fatal
}

/// Check an exported flow and report what is wrong with it.
///
/// Exit 1 when anything is reported as an error, 0 when only warnings are,
/// so this can gate a commit without a stylistic quibble blocking one.
fn run_check(args: &Args) {
    let input_path = Path::new(&args.path);
    let input = match decoder::load_flow_json(input_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("paxc: cannot read {}: {e}", args.path);
            process::exit(2);
        }
    };
    let mut findings = match check::check_flow(&input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("paxc: cannot check {}: {e}", args.path);
            process::exit(2);
        }
    };

    // `--allow` means the same thing here as it does on a compile. A code you
    // can wave through in one mode and not the other would be a trap for
    // anyone running both, which is what a CI job that compiles and checks
    // does.
    for finding in &mut findings {
        if args.allow.iter().any(|a| a == finding.code) {
            finding.severity = check::Severity::Warning;
        }
    }

    if findings.is_empty() {
        eprintln!("{}: no problems found", args.path);
        return;
    }

    for finding in &findings {
        println!("{finding}");
    }
    let errors = findings
        .iter()
        .filter(|f| f.severity == check::Severity::Error)
        .count();
    let warnings = findings.len() - errors;
    eprintln!("{}: {errors} error(s), {warnings} warning(s)", args.path);
    if errors > 0 {
        process::exit(1);
    }
}

fn run_decode(args: &Args) {
    let input_path = Path::new(&args.path);
    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or_else(|| default_decode_out_dir(input_path));
    match decoder::decode_file(input_path, &out_dir) {
        Ok(report) => {
            for w in &report.warnings {
                eprintln!("{w}");
            }
            eprintln!("wrote {}", report.pax_path.display());
            for f in &report.pa_files_written {
                eprintln!("wrote {}", f.display());
            }
        }
        Err(e) => {
            eprintln!("paxc: decode failed: {e}");
            process::exit(1);
        }
    }
}

/// Returns the displayName from `<source_dir>/pa/flow.json` when present, so
/// the encode side can default `--name` to it. Silent on any failure (file
/// missing, malformed, etc.) — caller falls back to the input basename.
fn read_display_name(source_dir: Option<&Path>) -> Option<String> {
    let dir = source_dir?;
    let bytes = fs::read(dir.join("pa/flow.json")).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("displayName")?.as_str().map(str::to_string)
}

fn derive_name_from_path(path: &str) -> String {
    if path == "-" {
        return "flow".to_string();
    }
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("flow")
        .to_string()
}

/// Pick the default output directory for `--decode` when the user didn't
/// pass `--out-dir`. For a `.json` input, that's the input file's parent
/// directory (the existing behavior). For a `.zip` input, dropping a
/// `pa/` folder next to the zip would clutter the user's working dir, so
/// instead we sit a sister directory named after the zip's stem alongside
/// the zip itself (`MyFlow_2026.zip` → `MyFlow_2026/`).
fn default_decode_out_dir(input_path: &Path) -> PathBuf {
    let is_zip = input_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
    let parent = input_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if is_zip {
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("flow");
        parent.join(stem)
    } else {
        parent
    }
}
