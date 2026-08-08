//! The connector catalogue in `skills/paxc/connectors.md` is the one page an
//! agent copies JSON out of verbatim, so a body that has drifted, lost a
//! required host field, or picked up a tenant value is worse than no page at
//! all. This harness reads the catalogue the way a reader does — every fenced
//! `json` block introduced by a backticked `pa/...` path is one file's whole
//! contents — and then puts each one through the real compiler.
//!
//! Three things are checked. The bodies parse and carry the host fields PA's
//! importer requires. Each one survives lex → parse → resolve → package and
//! comes out of the packaged `definition.json` wired to its connection
//! reference, which is the property #15 and #19 were both about. And nothing
//! on the page looks like it came out of somebody's tenant.

use chumsky::prelude::*;
use paxc::pa::packager::{self, Target};
use paxc::{lexer, parser, resolver};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// One catalogue entry: the `pa/` filename it is introduced by, and the JSON
/// body that follows.
struct Entry {
    file: String,
    body: Value,
}

fn catalogue_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/paxc/connectors.md")
}

/// Pull every fenced `json` block out of the catalogue, tagging the ones
/// introduced by a line that is nothing but a backticked `pa/...` path. An
/// untagged block is illustrative (the bare envelope at the top) — still
/// parsed, since invalid JSON in an example is its own kind of wrong, but not
/// compiled.
fn parse_catalogue(md: &str) -> (Vec<Entry>, usize) {
    let mut entries = Vec::new();
    let mut untagged = 0;
    let mut pending_file: Option<String> = None;
    let mut fence: Option<Vec<&str>> = None;

    for line in md.lines() {
        match fence {
            Some(ref mut body) => {
                if line.trim_start().starts_with("```") {
                    let text = body.join("\n");
                    let value: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
                        panic!(
                            "a json block in connectors.md does not parse ({e})\n\
                             introduced by: {}\n{text}",
                            pending_file.as_deref().unwrap_or("(untagged)")
                        )
                    });
                    match pending_file.take() {
                        Some(file) => entries.push(Entry { file, body: value }),
                        None => untagged += 1,
                    }
                    fence = None;
                } else {
                    body.push(line);
                }
            }
            None => {
                if line.trim() == "```json" {
                    fence = Some(Vec::new());
                } else if let Some(rest) = line.trim().strip_prefix("`pa/") {
                    // A line that is only a backticked path names the file the
                    // next block belongs in. Prose that merely mentions a path
                    // has something after the closing backtick and is skipped.
                    if let Some(name) = rest.strip_suffix('`') {
                        pending_file = Some(format!("pa/{name}"));
                    }
                } else if !line.trim().is_empty() && pending_file.is_some() {
                    // Anything but blank lines between the path and its block
                    // means the pairing was accidental.
                    pending_file = None;
                }
            }
        }
    }
    assert!(fence.is_none(), "unterminated json fence in connectors.md");
    (entries, untagged)
}

fn tmp_dir(label: &str) -> PathBuf {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let p = std::env::temp_dir().join(format!("paxc-catalogue-{safe}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(p.join("pa")).unwrap();
    p
}

/// Compile a one-file source tree through the real pipeline and read the
/// flow definition back out of the packaged zip.
fn package_and_read_definition(dir: &Path, label: &str) -> Value {
    let pax_path = dir.join("flow.pax");
    let src = fs::read_to_string(&pax_path).unwrap();
    let tokens = lexer::lexer()
        .parse(src.as_str())
        .into_result()
        .unwrap_or_else(|e| panic!("{label}: lex failed: {e:?}"));
    let program = parser::parser()
        .parse(
            tokens
                .as_slice()
                .map((src.len()..src.len()).into(), |(t, s)| (t, s)),
        )
        .into_result()
        .unwrap_or_else(|e| panic!("{label}: parse failed: {e:?}"));
    let resolved = resolver::resolve(&program, pax_path.parent())
        .unwrap_or_else(|e| panic!("{label}: resolve failed: {e:?}"));

    let zip_path = dir.join("flow.zip");
    packager::package(&resolved, Target::PaLegacy, "catalogue", &zip_path)
        .unwrap_or_else(|e| panic!("{label}: package failed: {e:?}"));

    let mut archive = zip::ZipArchive::new(fs::File::open(&zip_path).unwrap()).unwrap();
    let name = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .find(|n| n.ends_with("definition.json"))
        .unwrap_or_else(|| panic!("{label}: no definition.json in the package"));
    let mut text = String::new();
    archive
        .by_name(&name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The API name a body's `apiId` ends in — `shared_sharepointonline` and so on.
fn api_name(body: &Value) -> Option<String> {
    body["inputs"]["host"]["apiId"]
        .as_str()?
        .rsplit('/')
        .next()
        .map(str::to_string)
}

#[test]
fn every_catalogue_body_carries_the_host_fields_the_importer_needs() {
    let md = fs::read_to_string(catalogue_path()).expect("read connectors.md");
    let (entries, untagged) = parse_catalogue(&md);
    assert!(
        entries.len() >= 10,
        "the catalogue should cover the common connectors; found {} entries",
        entries.len()
    );
    assert!(untagged > 0, "the envelope example should still be there");

    for Entry { file, body } in &entries {
        if file.ends_with("connectionReferences.json") {
            for (key, reference) in body.as_object().expect("connectionReferences is a map") {
                let id = reference["id"].as_str().unwrap_or_default();
                assert!(
                    id.ends_with(key),
                    "{file}: the key `{key}` should be the last segment of its `id` ({id}); \
                     that key is what a body's host.connectionName has to match"
                );
            }
            continue;
        }

        let kind = body["type"].as_str().unwrap_or_default();
        assert!(
            kind.starts_with("OpenApiConnection"),
            "{file}: type `{kind}` is not a connector type"
        );

        let host = &body["inputs"]["host"];
        for field in ["apiId", "connectionName", "operationId"] {
            assert!(
                host[field].is_string(),
                "{file}: host.{field} is missing, and PA needs all three"
            );
        }
        assert_eq!(
            host["connectionName"].as_str(),
            api_name(body).as_deref(),
            "{file}: host.connectionName should be the api name from apiId, \
             which is the convention connectionReferences.json is keyed by"
        );

        // The two fields the packager owns. A body that ships either one is
        // teaching the reader to write what paxc is there to handle.
        assert!(
            body["inputs"].get("authentication").is_none(),
            "{file}: `inputs.authentication` is stripped at package time; \
             it should not be in the catalogue"
        );
        assert!(
            host.get("connectionReferenceName").is_none(),
            "{file}: `host.connectionReferenceName` is added at package time; \
             it should not be in the catalogue"
        );
    }
}

#[test]
fn every_catalogue_body_compiles_and_comes_out_wired_to_its_connection() {
    let md = fs::read_to_string(catalogue_path()).expect("read connectors.md");
    let (entries, _) = parse_catalogue(&md);

    for Entry { file, body } in &entries {
        if file.ends_with("connectionReferences.json") {
            continue;
        }
        let stem = file
            .trim_start_matches("pa/")
            .trim_end_matches(".json")
            .trim_end_matches(".trigger");
        let is_trigger = file.ends_with(".trigger.json");
        let dir = tmp_dir(stem);

        // Each body is compiled on its own, against a connection reference
        // map synthesized from the body itself, so the entry is checked
        // rather than the catalogue's example map.
        let api = api_name(body).expect("apiId");
        fs::write(
            dir.join("pa").join("connectionReferences.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                &api: { "id": body["inputs"]["host"]["apiId"], "apiName": api }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("pa").join(file.trim_start_matches("pa/")),
            serde_json::to_string_pretty(body).unwrap(),
        )
        .unwrap();
        // A trigger is found by its filename; an action has to be named.
        let source = if is_trigger {
            "var ok: int = 1\n".to_string()
        } else {
            format!("pa {stem}\n")
        };
        fs::write(dir.join("flow.pax"), source).unwrap();

        let definition = package_and_read_definition(&dir, file);
        let map = if is_trigger { "triggers" } else { "actions" };
        let emitted = &definition["properties"]["definition"][map][stem];
        assert!(
            !emitted.is_null(),
            "{file}: nothing named `{stem}` in the packaged {map}"
        );
        assert_eq!(
            emitted["inputs"]["host"]["connectionReferenceName"].as_str(),
            Some(api.as_str()),
            "{file}: the packaged body should be wired to its connection reference"
        );
        assert!(
            emitted["inputs"].get("authentication").is_none(),
            "{file}: the packaged body should not carry authentication"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_catalogue_carries_no_tenant_values() {
    let md = fs::read_to_string(catalogue_path()).expect("read connectors.md");

    // Every host in the catalogue should be Microsoft's own or the example
    // domain. A real site URL is the likeliest way tenant data would arrive.
    for host in md.split("https://").skip(1) {
        let host: String = host
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '`')
            .collect();
        assert!(
            host.ends_with("contoso.sharepoint.com")
                || host.ends_with("microsoft.com")
                || host.ends_with("office.com"),
            "connectors.md points at `{host}`; catalogue examples use contoso.sharepoint.com"
        );
    }

    // Connection ids, form ids and site ids are all GUIDs, and none of them
    // belongs on this page -- placeholders stand in for every one of them.
    let guid = |s: &str| {
        let b = s.as_bytes();
        b.len() == 36
            && b.iter().enumerate().all(|(i, c)| match i {
                8 | 13 | 18 | 23 => *c == b'-',
                _ => c.is_ascii_hexdigit(),
            })
    };
    for word in md.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        assert!(!guid(word), "connectors.md contains a GUID: {word}");
    }
}
