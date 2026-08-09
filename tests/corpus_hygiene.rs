//! The gate that decides whether `tests/corpus/` may leave this machine.
//!
//! The corpus exists to test the decoder against PA JSON a human really built
//! in the designer, which is the one thing hand-written fixtures cannot fake.
//! The flows that gave it that quality came out of a client tenant, so every
//! value naming that tenant has to be rewritten before the directory goes
//! anywhere. These tests are what "has to be" means: they fail on any corpus
//! file still carrying a real host, address, or identifier.
//!
//! Two layers, not one. The scrub is the first and the load-bearing one. The
//! second is that the scrubbed corpus lives in `excelano/paxc-testing`, which
//! is private, and reaches `tests/corpus/` here by clone; the directory stays
//! gitignored in this repo. Neither layer is asked to work alone.
//!
//! The decoder never reads any of it. It branches on the action `type`, on the
//! *shape* of an action key, on the `runAfter` graph, on the structure of an
//! `expression`, and on whether a `foreach` expression parses. A site URL and a
//! form id are inert to all of that, which is why they can be replaced without
//! costing the corpus a single case it used to cover.
//!
//! ## What these tests can and cannot prove
//!
//! They prove the absence of the five things a machine can recognise on sight:
//! hostnames, email addresses, GUIDs, the undashed hex identifiers Forms gives
//! its questions, and the base64 identifiers SharePoint and OneDrive give
//! drives and items. They cannot prove the absence
//! of business vocabulary -- a list named after a client project, a column
//! named after an internal process -- because there is no pattern that
//! separates that from any other string. Renaming it is a judgement call made
//! by a reader, recorded in the testing repo's README, and not enforceable
//! here.
//!
//! Each of the five started as a class somebody noticed, and the last two were
//! noticed only by reading the corpus after the first three passed. A guard
//! that has never been shown the thing it is meant to catch is a guess; run a
//! new class of file past all five before trusting them on it.
//!
//! `corpus_avoids_the_configured_denylist` closes part of that gap without
//! naming anyone in a committed file: point `PAXC_CORPUS_DENYLIST` at a local
//! list of the original strings and it fails on any survivor. The list stays
//! off GitHub, so the check is thorough locally and absent in CI, which is the
//! right way round.

use std::fs;
use std::path::PathBuf;

/// Hosts a scrubbed corpus is allowed to name.
///
/// `contoso` is Microsoft's own example tenant, which is what makes it the
/// safe substitute rather than merely a fake-sounding one. The schema and
/// APIM hosts are Microsoft infrastructure that appears in every export and
/// identifies nobody.
const ALLOWED_HOST_SUFFIXES: &[&str] = &[
    "contoso.sharepoint.com",
    "contoso.com",
    "forms.office.com",
    "schema.management.azure.com",
    "azure-apim.net",
];

/// Domains a scrubbed corpus is allowed to send mail to.
const ALLOWED_MAIL_DOMAINS: &[&str] = &["contoso.com", "contoso.onmicrosoft.com"];

/// Every GUID in a scrubbed corpus is minted from a counter, so it is
/// recognisable as synthetic at a glance and needs no allowlist to maintain.
/// The variant and version nibbles are real (`4`, `8`) so the value is still a
/// well-formed UUID; the counter occupies the last four hex digits, which is
/// room for far more distinct ids than a corpus will ever hold.
///
/// Distinctness is preserved across the scrub: two originals that differed
/// still differ afterwards, because operationMetadataId and connection names
/// have to stay unique for the round-trip to mean anything.
const SYNTHETIC_GUID_PREFIX: &str = "00000000-0000-4000-8000-00000000";

/// The same idea for identifiers that carry no dashes.
///
/// Microsoft Forms names its questions `r` followed by 32 hex characters, and
/// SharePoint uses the undashed form for some ids too. None of them matches the
/// GUID shape, so a guard that only knew about GUIDs would pass a corpus with
/// dozens of real question ids still in it -- which is exactly what this one
/// did until the scrub turned them up.
const SYNTHETIC_HEX_PREFIX: &str = "0000000000000000000000000000";

/// Shortest identifier the undashed check considers. Below this, hex runs are
/// ordinary content: colour codes, a truncated hash, a version fragment.
const HEX_ID_MIN_LEN: usize = 32;

/// And the same idea again for identifiers that are not hex at all.
///
/// SharePoint writes a drive id as `b!` and 64 base64url characters, OneDrive
/// writes an item id as `01` and 32 base32 ones, and a Forms form id runs to
/// ninety. Every one of them encodes a site or list GUID, so every one is
/// tenant data, and none matches either of the checks above. The scrub mints
/// replacements starting with this prefix.
const SYNTHETIC_OPAQUE_PREFIX: &str = "0000";

/// Shortest run the opaque check considers.
const OPAQUE_ID_MIN_LEN: usize = 32;

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

/// Every file under `tests/corpus/`, as (display path, contents).
///
/// Returns empty when the directory is absent so a checkout without a corpus
/// runs green rather than red. `round_trip_corpus` is what insists a corpus
/// exists; this module only insists that whatever exists is clean.
fn corpus_files() -> Vec<(String, String)> {
    fn walk(dir: &PathBuf, out: &mut Vec<(String, String)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Ok(text) = fs::read_to_string(&path) {
                let label = path
                    .strip_prefix(corpus_root())
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                out.push((label, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(&corpus_root(), &mut out);
    out.sort();
    out
}

/// The authority of every `scheme://host...` in the text.
fn hosts(text: &str) -> Vec<String> {
    text.match_indices("://")
        .map(|(i, _)| {
            text[i + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|h| !h.is_empty())
        .collect()
}

/// Anything shaped like an address. Deliberately loose: over-reporting here
/// costs one rename, while under-reporting publishes a real address.
fn email_addresses(text: &str) -> Vec<String> {
    let addr_char = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let domain_char = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    let bytes: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for (i, c) in bytes.iter().enumerate() {
        if *c != '@' {
            continue;
        }
        let start = bytes[..i]
            .iter()
            .rposition(|c| !addr_char(*c))
            .map_or(0, |p| p + 1);
        let end = bytes[i + 1..]
            .iter()
            .position(|c| !domain_char(*c))
            .map_or(bytes.len(), |p| i + 1 + p);
        if start == i || end == i + 1 {
            continue;
        }
        let domain: String = bytes[i + 1..end].iter().collect();
        // PA expressions are full of `@{...}` and `@outputs(...)`; only a
        // dotted right-hand side is plausibly an address.
        if domain.contains('.') && !domain.ends_with('.') {
            found.push(
                bytes[start..end]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
        }
    }
    found
}

fn is_guid(s: &str) -> bool {
    guid_shaped(s.as_bytes())
}

fn guid_shaped(w: &[u8]) -> bool {
    w.len() == 36
        && w.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Every GUID in the text, including ones embedded in a longer token.
///
/// A sliding window rather than a tokeniser, because PA buries them: a
/// connection name reads `shared-sharepointonl-<guid>`, which splits into one
/// word on any delimiter set that keeps hyphens, and drops the GUID entirely on
/// any set that does not. The window cannot miss either way.
fn guids(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut found = Vec::new();
    for start in 0..b.len().saturating_sub(35) {
        let window = &b[start..start + 36];
        if guid_shaped(window) {
            found.push(String::from_utf8_lossy(window).to_ascii_lowercase());
        }
    }
    found
}

/// Every maximal run of hex characters at least `HEX_ID_MIN_LEN` long.
///
/// Maximal so that the `r` Forms prefixes its question ids with does not shift
/// the window and hide the run behind a false start.
fn hex_identifiers(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_hexdigit() && !c.is_ascii_uppercase() {
            run.push(c);
        } else {
            if run.len() >= HEX_ID_MIN_LEN {
                found.push(std::mem::take(&mut run));
            }
            run.clear();
        }
    }
    found
}

/// Every run of at least `OPAQUE_ID_MIN_LEN` base64url characters that could be
/// an encoded identifier.
///
/// Synthetic GUIDs are blanked first, because a connection name reads
/// `shared-sharepointonl-<guid>` and would otherwise present as one 56
/// character run that no rule below could explain.
///
/// Underscores split a run, so an action key like
/// `if_Operations_OR_Operations_Leadership` is several short tokens rather than
/// one long one. What is left has to look like an identifier and not like the
/// two other things of that length and character class: a hostname such as
/// `flow-apim-unitedstates-002-westus-01`, which has no upper case, and a
/// schema field name such as `isProcessSimpleApiReferenceConversionAlreadyDone`,
/// which has no digits. Requiring both leaves the real ones.
fn opaque_identifiers(text: &str) -> Vec<String> {
    let blanked = guids(text).iter().fold(text.to_string(), |acc, g| {
        acc.replace(g, " ").replace(&g.to_ascii_uppercase(), " ")
    });
    let mut found = Vec::new();
    let mut run = String::new();
    for c in blanked.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '-' {
            run.push(c);
        } else {
            let long = run.len() >= OPAQUE_ID_MIN_LEN;
            let upper = run.chars().any(|c| c.is_ascii_uppercase());
            let digit = run.chars().any(|c| c.is_ascii_digit());
            if long && upper && digit {
                found.push(std::mem::take(&mut run));
            }
            run.clear();
        }
    }
    found
}

#[test]
fn corpus_hosts_are_all_fictional() {
    let mut bad = Vec::new();
    for (file, text) in corpus_files() {
        for host in hosts(&text) {
            if !ALLOWED_HOST_SUFFIXES.iter().any(|s| host.ends_with(s)) {
                bad.push(format!("{file}: {host}"));
            }
        }
    }
    bad.dedup();
    assert!(
        bad.is_empty(),
        "corpus files name hosts outside the allowlist -- a real site URL is the \
         likeliest way tenant data reaches a commit:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn corpus_email_addresses_are_all_fictional() {
    let mut bad = Vec::new();
    for (file, text) in corpus_files() {
        for addr in email_addresses(&text) {
            let domain = addr.split('@').nth(1).unwrap_or_default().to_string();
            if !ALLOWED_MAIL_DOMAINS.contains(&domain.as_str()) {
                bad.push(format!("{file}: {addr}"));
            }
        }
    }
    bad.dedup();
    assert!(
        bad.is_empty(),
        "corpus files carry addresses outside {ALLOWED_MAIL_DOMAINS:?}:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn corpus_guids_are_all_synthetic() {
    let mut bad = Vec::new();
    for (file, text) in corpus_files() {
        for guid in guids(&text) {
            if !guid.starts_with(SYNTHETIC_GUID_PREFIX) {
                bad.push(format!("{file}: {guid}"));
            }
        }
    }
    bad.dedup();
    let count = bad.len();
    assert!(
        bad.is_empty(),
        "{count} GUID(s) in the corpus were not minted by the scrub; every one \
         should read {SYNTHETIC_GUID_PREFIX}xxxx:\n  {}",
        bad.iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn corpus_hex_identifiers_are_all_synthetic() {
    let mut bad = Vec::new();
    for (file, text) in corpus_files() {
        for id in hex_identifiers(&text) {
            if !id.starts_with(SYNTHETIC_HEX_PREFIX) {
                bad.push(format!("{file}: {id}"));
            }
        }
    }
    bad.dedup();
    let count = bad.len();
    assert!(
        bad.is_empty(),
        "{count} undashed hex identifier(s) in the corpus were not minted by the \
         scrub -- Forms question ids look like this and are tenant data:\n  {}",
        bad.iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn corpus_opaque_identifiers_are_all_synthetic() {
    let mut bad = Vec::new();
    for (file, text) in corpus_files() {
        for id in opaque_identifiers(&text) {
            if !id.starts_with(SYNTHETIC_OPAQUE_PREFIX) {
                bad.push(format!("{file}: {id}"));
            }
        }
    }
    bad.dedup();
    let count = bad.len();
    assert!(
        bad.is_empty(),
        "{count} base64 identifier(s) in the corpus were not minted by the scrub \
         -- a SharePoint drive id and a Forms form id both encode the site and \
         list they belong to:\n  {}",
        bad.iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// Catch what a pattern cannot: the client's own vocabulary.
///
/// Set `PAXC_CORPUS_DENYLIST` to a file of forbidden substrings, one per line,
/// blank lines and `#` comments ignored. Matching is case-insensitive. The file
/// is never committed, which is the point -- it can name the things a public
/// test file must not.
#[test]
fn corpus_avoids_the_configured_denylist() {
    let Some(path) = std::env::var_os("PAXC_CORPUS_DENYLIST") else {
        eprintln!("PAXC_CORPUS_DENYLIST not set; skipping the vocabulary check");
        return;
    };
    let list = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("PAXC_CORPUS_DENYLIST points at {path:?}, which cannot be read: {e}")
    });
    let terms: Vec<String> = list
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_ascii_lowercase())
        .collect();
    assert!(
        !terms.is_empty(),
        "PAXC_CORPUS_DENYLIST points at {path:?}, which holds no terms -- an empty \
         denylist passes silently and is worse than none"
    );

    let mut hits = Vec::new();
    for (file, text) in corpus_files() {
        let lowered = text.to_ascii_lowercase();
        for term in &terms {
            if lowered.contains(term.as_str()) {
                hits.push(format!("{file}: {term}"));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "{} denylisted term(s) survive in the corpus:\n  {}",
        hits.len(),
        hits.join("\n  ")
    );
}

// The scanners above are the whole of this gate, so they get their own tests.
// A guard that silently stopped recognising a site URL would report a clean
// corpus in exactly the case it exists to catch.

#[test]
fn hosts_are_read_out_of_a_url() {
    let found = hosts("see https://contoso.sharepoint.com/sites/X and http://Forms.office.com/y");
    assert_eq!(found, vec!["contoso.sharepoint.com", "forms.office.com"]);
}

#[test]
fn a_pa_expression_is_not_mistaken_for_an_address() {
    // `@{...}`, `@outputs(...)` and `@body(...)` are everywhere in PA JSON.
    let found = email_addresses("@{outputs('X')?['body/value']} and @body('Y')");
    assert!(found.is_empty(), "expressions read as addresses: {found:?}");
}

#[test]
fn an_address_is_found_and_lowercased() {
    let found = email_addresses(r#""to": "First.Last@Contoso.com","#);
    assert_eq!(found, vec!["first.last@contoso.com"]);
}

#[test]
fn a_synthetic_guid_is_distinguishable_from_a_real_one() {
    let synthetic = format!("{SYNTHETIC_GUID_PREFIX}0001");
    assert!(
        is_guid(&synthetic),
        "the synthetic form must be a valid GUID"
    );
    assert!(synthetic.starts_with(SYNTHETIC_GUID_PREFIX));
    assert!(!"40de3cb3-fa02-4678-954b-d14391b7b0ec".starts_with(SYNTHETIC_GUID_PREFIX));
}

/// Forms writes `r` then 32 hex characters, which is neither a GUID nor
/// separable by a word boundary from the letter in front of it.
#[test]
fn a_forms_question_id_is_recognised_despite_its_prefix() {
    let found = hex_identifiers(r#""body/r075cc5abe2894d028d4d934e8e57ece5": "yes""#);
    assert_eq!(found, vec!["075cc5abe2894d028d4d934e8e57ece5"]);
}

#[test]
fn short_hex_runs_are_left_alone() {
    // A colour, a truncated sha, a version fragment: all ordinary content.
    assert!(hex_identifiers("#ff00cc and abc123 and deadbeef").is_empty());
}

#[test]
fn a_sharepoint_drive_id_is_recognised() {
    let found = opaque_identifiers(
        r#""b!9luAMdXbbEWDUm3yeUnQ-pFGdxG4XDpIvM06C_I-8m7engY4h0IxQrfAIIiZBcHT": "/Documents""#,
    );
    assert!(
        found.iter().any(|id| id.starts_with("9luAMdXb")),
        "the drive id was not found: {found:?}"
    );
}

/// The two things that share the drive id's length and character class and are
/// not identifiers. Flagging either would make the guard cry wolf on every
/// export, since both appear in all of them.
#[test]
fn an_azure_hostname_is_not_mistaken_for_an_identifier() {
    let found = opaque_identifiers("https://flow-apim-unitedstates-002-westus-01.azure-apim.net");
    assert!(
        found.is_empty(),
        "hostname read as an identifier: {found:?}"
    );
}

#[test]
fn a_schema_field_name_is_not_mistaken_for_an_identifier() {
    let found = opaque_identifiers(r#""isProcessSimpleApiReferenceConversionAlreadyDone": false"#);
    assert!(
        found.is_empty(),
        "schema field read as an identifier: {found:?}"
    );
}

/// A synthetic GUID inside a connection name must not be mistaken for one long
/// opaque token, or the guard fails on a corpus it has already cleared.
#[test]
fn a_scrubbed_connection_name_holds_no_opaque_identifier() {
    let found = opaque_identifiers(&format!(
        r#""connectionName": "shared-sharepointonl-{SYNTHETIC_GUID_PREFIX}0007""#
    ));
    assert!(found.is_empty(), "clean connection name flagged: {found:?}");
}

/// The case a tokeniser gets wrong: PA writes connection names as
/// `shared-<api>-<guid>`, so the id is welded to a prefix by hyphens.
#[test]
fn a_guid_welded_into_a_connection_name_is_still_found() {
    let found =
        guids(r#""connectionName": "shared-sharepointonl-00000000-0000-4000-8000-000000000007""#);
    assert_eq!(found, vec![format!("{SYNTHETIC_GUID_PREFIX}0007")]);
}
