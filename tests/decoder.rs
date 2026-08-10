//! Corpus-driven round-trip harness for the PA JSON decoder.
//!
//! For each `tests/corpus/<name>/input.json` the harness:
//!   1. Decodes the PA flow JSON into pax + a `pa/` folder under a tempdir.
//!   2. Compiles the decoded pax back through the lex → parse → resolve →
//!      emit pipeline to produce a fresh PA flow JSON.
//!   3. Asserts the re-emitted JSON matches the original at the level of
//!      structural / semantic equivalence (see `compare_definitions` for
//!      exactly what's compared and what's intentionally tolerated).
//!
//!   4. Checks the decoder's own notes against a recorded snapshot, which is
//!      the only part of this that can see a lowering regression: an action
//!      that stops being lowered still round-trips, as a `pa` block.
//!
//! Adding a new flow to the corpus is dropping a `definition.json` at
//! `tests/corpus/<descriptive-name>/input.json` and then running the harness
//! twice — the first run records `decode-notes.txt` and fails, so the list of
//! actions the decoder gave up on gets read by a person before it becomes the
//! baseline. The harness reports every divergence it found rather than the
//! first, so one run triages a whole corpus.

use chumsky::prelude::*;
use paxc::pa::{decoder, emitter};
use paxc::{lexer, parser, resolver};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn corpus_entries() -> Vec<PathBuf> {
    let root = corpus_root();
    if !root.exists() {
        return Vec::new();
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(&root)
        .expect("read tests/corpus")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("input.json").exists())
        .collect();
    entries.sort();
    entries
}

fn tmp_dir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("paxc-corpus-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn compile_pax_to_definition(pax_path: &Path) -> Value {
    let src = fs::read_to_string(pax_path).expect("read decoded pax");
    let tokens = lexer::lexer()
        .parse(src.as_str())
        .into_result()
        .expect("lex");
    let program = parser::parser()
        .parse(
            tokens
                .as_slice()
                .map((src.len()..src.len()).into(), |(t, s)| (t, s)),
        )
        .into_result()
        .expect("parse");
    let source_dir = pax_path.parent();
    let resolved = resolver::resolve(&program, source_dir).expect("resolve");
    emitter::emit(&resolved)
}

/// Set this to let the test pass when no corpus is present at all. It exists
/// for one caller, `ci.yml`, where the corpus is deliberately absent: it lives
/// in a private repo and fetching it onto a public repo's runner would cost a
/// long-lived credential to re-run a test that already runs on the machine
/// holding the corpus. Everywhere else a missing corpus is a broken checkout
/// and should say so, which is why the skip is asked for rather than inferred.
const ALLOW_MISSING_CORPUS: &str = "PAXC_ALLOW_MISSING_CORPUS";

#[test]
fn round_trip_corpus() {
    let root = corpus_root();
    if !root.exists() {
        assert!(
            std::env::var_os(ALLOW_MISSING_CORPUS).is_some(),
            "no corpus at {}. It lives in the private excelano/paxc-testing; \
             clone or symlink it into place. Set {ALLOW_MISSING_CORPUS} to run \
             the rest of the suite without it.",
            root.display()
        );
        eprintln!(
            "skipping round_trip_corpus: no corpus at {} and {ALLOW_MISSING_CORPUS} is set",
            root.display()
        );
        return;
    }

    // A corpus that exists but holds nothing is a different problem from one
    // that was never fetched, and no env var excuses it.
    let entries = corpus_entries();
    assert!(
        !entries.is_empty(),
        "corpus at {} holds no <name>/input.json",
        root.display()
    );

    let mut failed: Vec<String> = Vec::new();
    for entry in entries {
        let label = entry.file_name().unwrap().to_string_lossy().to_string();
        let input_path = entry.join("input.json");
        let original_bytes = fs::read(&input_path).expect("read corpus input");
        let original: Value = serde_json::from_slice(&original_bytes).expect("parse corpus input");

        let out_dir = tmp_dir(&label);
        let report = match decoder::decode_file(&input_path, &out_dir) {
            Ok(r) => r,
            Err(e) => {
                failed.push(format!("{label}: decode failed: {e}"));
                continue;
            }
        };

        if let Err(diff) = check_decode_notes(&entry, &report.warnings) {
            failed.push(format!("{label}: {diff}"));
        }

        // The decoded pax file is named after the input stem ("input.pax").
        let pax_path = out_dir.join("input.pax");
        let reemitted = compile_pax_to_definition(&pax_path);

        if let Err(diff) = compare_definitions(&original, &reemitted) {
            failed.push(format!("{label}: {diff}"));
        }
    }

    if !failed.is_empty() {
        panic!(
            "{} corpus flow(s) did not round-trip cleanly:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }
}

/// Snapshot of what the decoder said while reading a corpus flow, chiefly its
/// record of every action it declined to lower natively and why.
///
/// The round-trip comparison cannot see this. An action that stops being
/// lowered falls back to a `pa` block, which re-emits verbatim and keeps the
/// same type, so a decoder that quietly gives up on lowering altogether still
/// round-trips perfectly. That is the opposite of what the corpus is for. The
/// notes are the decoder's own account of the judgement, and pinning them
/// turns a silent loss into a diff somebody has to approve.
///
/// A flow with no file yet gets one written from the current run and fails
/// once, so a new corpus entry cannot skip the check by omission. Regenerate
/// all of them deliberately with `PAXC_UPDATE_DECODE_NOTES=1 cargo test`.
const DECODE_NOTES_FILE: &str = "decode-notes.txt";

fn check_decode_notes(entry: &Path, warnings: &[String]) -> Result<(), String> {
    let path = entry.join(DECODE_NOTES_FILE);
    let actual = warnings.join("\n");
    let write = |text: &str| {
        let body = if text.is_empty() {
            String::new()
        } else {
            format!("{text}\n")
        };
        fs::write(&path, body).map_err(|e| format!("writing {}: {e}", path.display()))
    };

    if std::env::var_os("PAXC_UPDATE_DECODE_NOTES").is_some() {
        return write(&actual);
    }

    let expected = match fs::read_to_string(&path) {
        Ok(s) => s.trim_end().to_string(),
        Err(_) => {
            write(&actual)?;
            return Err(format!(
                "no {DECODE_NOTES_FILE} yet, so one was written from this run. Read it, \
                 confirm every fallback in it is one you meant, and re-run."
            ));
        }
    };
    if expected == actual {
        return Ok(());
    }

    let before: Vec<&str> = expected.lines().collect();
    let after: Vec<&str> = actual.lines().collect();
    let mut lines = Vec::new();
    for line in &after {
        if !before.contains(line) {
            lines.push(format!("  new:  {line}"));
        }
    }
    for line in &before {
        if !after.contains(line) {
            lines.push(format!("  gone: {line}"));
        }
    }
    Err(format!(
        "decode notes changed ({} recorded, {} now). If the change is intended, \
         regenerate with PAXC_UPDATE_DECODE_NOTES=1.\n{}",
        before.len(),
        after.len(),
        lines.join("\n")
    ))
}

/// Compare original PA export JSON against paxc's re-emitted JSON. Structural
/// equivalence, with these intentional tolerances:
///
/// * **Envelope fields** (`name`, `id`, `properties.apiId`, `properties.displayName`,
///   `properties.connectionReferences`, etc.): the original has them, paxc's
///   raw JSON output does not (paxc's envelope-building lives in the packager,
///   not the emitter). We compare only the inner `definition` block.
/// * **`metadata` block** inside `definition`: optional in PA; paxc omits it.
///   Stripped from the original before comparing.
/// * **`parameters` block** with `$authentication` / `$connections`: paxc
///   adds these only inside the packager. The bare emitter output omits them.
///   Stripped from the original.
/// * **`$schema` differences**: paxc uses one canonical schema URL; some
///   exports use slightly different ones. Tolerated.
/// * **Action-level `metadata` blocks** (PA's per-action operationMetadataId):
///   not preserved through `pa <Name>` (the JSON file does carry it, so it
///   does round-trip when paxc reads the file back). Comparison is on the
///   inner `definition` after these are stripped.
/// * **Action key prefix differences**: an `Initialize_variable_<name>` from
///   PA designer vs paxc's `Initialize_<name>` would mismatch. For slice 44a
///   the natively-lowered InitializeVariable / SetVariable / etc. preserve
///   the original action key by going through `pa <Name>` when the PA key
///   doesn't match paxc's regenerated form. Native lowering with a different
///   resulting key is documented as a known divergence in the slice-44a plan
///   and tolerated here at the "actions are present and have the right
///   types" level rather than exact key equality.
///
/// Everything below the top level counts. A PA action name is unique across
/// the whole workflow — `runAfter` and `body('X')` both address by bare name
/// — so both sides are flattened into one key-to-body map before comparison
/// and depth stops mattering. Without that, a flow whose work sits inside a
/// Scope is checked at the level of "there is a Scope" and nothing else.
fn compare_definitions(original: &Value, reemitted: &Value) -> Result<(), String> {
    let orig_def = strip_definition(original)?;
    let new_def = strip_definition(reemitted)?;

    // Triggers must match exactly — they're emitted verbatim from the
    // .trigger.json file.
    let orig_triggers = orig_def.get("triggers").ok_or("original has no triggers")?;
    let new_triggers = new_def.get("triggers").ok_or("reemitted has no triggers")?;
    if orig_triggers != new_triggers {
        return Err(format!(
            "triggers differ\noriginal: {}\nreemitted: {}",
            serde_json::to_string_pretty(orig_triggers).unwrap(),
            serde_json::to_string_pretty(new_triggers).unwrap()
        ));
    }

    // Action set: every original action key should appear in the reemitted
    // definition either with the same key (pa-block fallback) or recognized
    // as natively-lowered (in which case we just check action count by type).
    let orig_actions = orig_def
        .get("actions")
        .and_then(Value::as_object)
        .ok_or("original has no actions")?;
    let new_actions = new_def
        .get("actions")
        .and_then(Value::as_object)
        .ok_or("reemitted has no actions")?;

    // Counts by type at the top level — looser than exact key equality but
    // enough to catch dropped or duplicated actions. Checked before the
    // flattened count so a top-level loss names itself instead of arriving
    // as one number off in a corpus-wide tally.
    let orig_types = action_type_counts(&flatten_one_level(orig_actions));
    let new_types = action_type_counts(&flatten_one_level(new_actions));
    if orig_types != new_types {
        return Err(format!(
            "top-level action type counts differ\noriginal: {orig_types:?}\nreemitted: {new_types:?}"
        ));
    }

    // The same count over every action at every depth. A container's own key
    // is regenerated by native lowering, so its children cannot be addressed
    // by path; they are addressed by name, which PA guarantees is unique
    // across the workflow.
    let orig_all = flatten_actions(orig_actions);
    let new_all = flatten_actions(new_actions);
    let orig_all_types = action_type_counts(&orig_all);
    let new_all_types = action_type_counts(&new_all);
    if orig_all_types != new_all_types {
        return Err(format!(
            "action type counts differ at some depth ({} original actions, {} reemitted)\noriginal: {orig_all_types:?}\nreemitted: {new_all_types:?}",
            orig_all.len(),
            new_all.len()
        ));
    }

    // For pa-block fallbacks (anything outside the native-lowering set),
    // the action JSON is copied verbatim into pa/<Name>.json and emitted
    // unchanged — those bodies should match key-for-key (modulo runAfter,
    // which paxc regenerates from source order). The native-lowering set
    // is everything the decoder may natively rewrite into pax constructs:
    // when it does, the action key is regenerated by paxc's emitter
    // (`Initialize_<name>`, `Condition`, `Apply_to_each`, `Compose_<name>`,
    // ...) and the action body is rebuilt structurally — neither matches
    // the original byte-for-byte. Type counts stay the same (still one If,
    // still one Foreach), and the per-action unit tests inside paxc cover
    // semantic equivalence.
    let native_types: std::collections::HashSet<&str> = [
        "InitializeVariable",
        "SetVariable",
        "IncrementVariable",
        "DecrementVariable",
        "AppendToStringVariable",
        "AppendToArrayVariable",
        "Compose",
        "If",
        "Foreach",
        "Until",
        "Switch",
        "Scope",
    ]
    .into_iter()
    .collect();

    for (key, orig_body) in &orig_all {
        let orig_type = orig_body.get("type").and_then(Value::as_str).unwrap_or("");
        if native_types.contains(orig_type) {
            // Native-lowering case is harder to compare directly because the
            // action key may have been regenerated (Initialize_variable_x →
            // Initialize_x). Skip per-action body diffing for these in 44a;
            // the type-count check above catches gross mismatches.
            continue;
        }
        // Fall-back actions: must appear in reemitted under the original key,
        // exactly. A key the decoder had to sanitise for pax is restored from
        // `pa/flow.json.actionNameMap` on the way back out, so accepting the
        // sanitised spelling here would be accepting the map's loss.
        let reemit_body = match new_all.get(key) {
            Some(b) => b,
            None => {
                let near = normalize_for_lookup(key);
                let hint = if new_all.contains_key(&near) {
                    format!(
                        " (found `{near}` instead — the sanitised spelling, so actionNameMap did not restore it)"
                    )
                } else {
                    String::new()
                };
                return Err(format!(
                    "action `{key}` (type {orig_type}) missing from reemitted definition{hint}"
                ));
            }
        };
        // The pa-block path emits the body verbatim, modulo the runAfter
        // map which paxc regenerates from source order. Compare everything
        // EXCEPT runAfter, plus an existence check on runAfter.
        let orig_body_clean = strip_run_after(orig_body);
        let reemit_body_clean = strip_run_after(reemit_body);
        if orig_body_clean != reemit_body_clean {
            return Err(format!(
                "pa-block action `{key}` body differs after round-trip\noriginal: {}\nreemitted: {}",
                serde_json::to_string_pretty(&orig_body_clean).unwrap(),
                serde_json::to_string_pretty(&reemit_body_clean).unwrap()
            ));
        }
    }

    Ok(())
}

fn strip_definition(v: &Value) -> Result<Value, String> {
    // Both shapes appear:
    //   - Original PA export envelope: { properties: { definition: {...} } }
    //   - paxc emitter output: { definition: {...} }
    let def = v
        .get("properties")
        .and_then(|p| p.get("definition"))
        .or_else(|| v.get("definition"))
        .ok_or("no definition block")?;
    let mut def = def.clone();
    if let Value::Object(map) = &mut def {
        // metadata is regenerated by PA on import; paxc omits it.
        map.remove("metadata");
        // parameters block ($authentication, $connections) is added by the
        // packager wrapper, not the bare emitter. Strip for comparison.
        map.remove("parameters");
        // $schema: tolerate exact value differences.
        map.remove("$schema");
        // contentVersion: also envelope-y; tolerate.
        map.remove("contentVersion");
    }
    Ok(def)
}

fn strip_run_after(v: &Value) -> Value {
    let mut v = v.clone();
    if let Value::Object(map) = &mut v {
        map.remove("runAfter");
        // PA designer adds per-action metadata.operationMetadataId; paxc
        // copies this through verbatim from the pa/ file, so it should
        // round-trip cleanly. No strip needed here.
    }
    v
}

fn action_type_counts(
    actions: &std::collections::BTreeMap<String, Value>,
) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for body in actions.values() {
        let t = body
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("<missing>")
            .to_string();
        *counts.entry(t).or_insert(0) += 1;
    }
    counts
}

/// The top-level actions as a plain map, so the shallow and the deep count
/// go through the same code and any difference between them is depth alone.
fn flatten_one_level(
    actions: &serde_json::Map<String, Value>,
) -> std::collections::BTreeMap<String, Value> {
    actions
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Every action in the definition, at every depth, keyed by name. PA requires
/// action names to be unique across a whole workflow, so flattening loses
/// nothing that the comparison needs and gains the ability to address a
/// nested action whose parent's key was regenerated.
fn flatten_actions(
    actions: &serde_json::Map<String, Value>,
) -> std::collections::BTreeMap<String, Value> {
    let mut out = std::collections::BTreeMap::new();
    collect_actions(actions, &mut out);
    out
}

fn collect_actions(
    actions: &serde_json::Map<String, Value>,
    out: &mut std::collections::BTreeMap<String, Value>,
) {
    for (key, body) in actions {
        out.insert(key.clone(), body.clone());
        for nested in nested_action_blocks(body) {
            collect_actions(nested, out);
        }
    }
}

/// The four places a Logic Apps action can hold child actions: the `actions`
/// map of a Scope / Foreach / Until / If-then, the `else` branch of an If,
/// and a Switch's `cases` and `default`.
fn nested_action_blocks(body: &Value) -> Vec<&serde_json::Map<String, Value>> {
    let mut blocks = Vec::new();
    for path in [
        vec!["actions"],
        vec!["else", "actions"],
        vec!["default", "actions"],
    ] {
        let mut here = Some(body);
        for step in path {
            here = here.and_then(|v| v.get(step));
        }
        if let Some(m) = here.and_then(Value::as_object) {
            blocks.push(m);
        }
    }
    if let Some(cases) = body.get("cases").and_then(Value::as_object) {
        for case in cases.values() {
            if let Some(m) = case.get("actions").and_then(Value::as_object) {
                blocks.push(m);
            }
        }
    }
    blocks
}

/// Mirror of `decoder::normalize_action_key`'s base normalization (for the
/// `used` set being empty — i.e. no collision-suffix). Used only to look up
/// fallback actions in the reemitted map.
fn normalize_for_lookup(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut prev_underscore = false;
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_underscore = ch == '_';
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Slice 44f integration check: a PA action key with characters outside
/// `[A-Za-z_][A-Za-z0-9_]*` (here `Send_an_email_(V2)`) decodes to a
/// pax-safe `pa Send_an_email_V2`, and re-encoding via the resolver/emitter
/// pipeline restores the original key — proving `pa/flow.json.actionNameMap`
/// is being read on the encode side.
#[test]
fn decode_then_encode_preserves_original_pa_action_key() {
    use serde_json::json;

    let input = json!({
        "properties": {
            "displayName": "Name Map Round-Trip",
            "definition": {
                "$schema": "https://schema.management.azure.com/providers/Microsoft.Logic/schemas/2016-06-01/workflowdefinition.json#",
                "contentVersion": "1.0.0.0",
                "triggers": { "manual": { "type": "Request", "kind": "Button", "inputs": {} } },
                "actions": {
                    "Send_an_email_(V2)": {
                        "type": "OpenApiConnection",
                        "runAfter": {},
                        "inputs": { "method": "POST" }
                    }
                }
            }
        }
    });

    let dir = tmp_dir("namemap_roundtrip");
    let input_path = dir.join("input.json");
    fs::write(&input_path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();

    let _report = decoder::decode_file(&input_path, &dir).expect("decode");

    // The decoded pax should reference the safe name.
    let pax = fs::read_to_string(dir.join("input.pax")).unwrap();
    assert!(
        pax.contains("pa Send_an_email_V2"),
        "decoded pax should use the pax-safe name, got: {pax}"
    );

    // pa/flow.json should carry the actionNameMap.
    let flow_meta: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pa/flow.json")).unwrap()).unwrap();
    assert_eq!(
        flow_meta["actionNameMap"]["Send_an_email_(V2)"],
        "Send_an_email_V2"
    );

    // Re-encode through the resolver/emitter pipeline. The resolver reads
    // actionNameMap and overrides the emit name back to the original.
    let reemitted = compile_pax_to_definition(&dir.join("input.pax"));
    let actions = reemitted["definition"]["actions"]
        .as_object()
        .expect("actions object");
    assert!(
        actions.contains_key("Send_an_email_(V2)"),
        "expected re-emit to restore original PA key; got keys: {:?}",
        actions.keys().collect::<Vec<_>>()
    );
    assert!(
        !actions.contains_key("Send_an_email_V2"),
        "the pax-safe name should NOT appear in the re-emitted JSON; got keys: {:?}",
        actions.keys().collect::<Vec<_>>()
    );
}

/// Issue #25: a designer-shaped `Compose_<id>` whose expression calls the
/// URI-component functions must collapse to `let <id> = ...` rather than
/// falling back to an opaque `pa` block. The corpus happens to cover this
/// through one flow that calls `decodeUriComponent`, but corpus contents are
/// refreshed from a live tenant and can stop covering it without anyone
/// noticing, so the property gets a fixture that does not depend on them.
#[test]
fn compose_calling_uri_component_functions_lowers_to_a_let() {
    use serde_json::json;

    let expr = "@replace(variables('raw'), decodeUriComponent('%0D%0A%0D%0A'), \
                encodeUriComponent('%0D%0A'))";
    let input = json!({
        "properties": {
            "displayName": "URI Component Round-Trip",
            "definition": {
                "$schema": "https://schema.management.azure.com/providers/Microsoft.Logic/schemas/2016-06-01/workflowdefinition.json#",
                "contentVersion": "1.0.0.0",
                "triggers": { "manual": { "type": "Request", "kind": "Button", "inputs": {} } },
                "actions": {
                    "Initialize_variable_raw": {
                        "type": "InitializeVariable",
                        "runAfter": {},
                        "inputs": { "variables": [ { "name": "raw", "type": "string", "value": "" } ] }
                    },
                    "Compose_Cleaned": {
                        "type": "Compose",
                        "runAfter": { "Initialize_variable_raw": ["Succeeded"] },
                        "inputs": expr
                    }
                }
            }
        }
    });

    let dir = tmp_dir("uri_component_roundtrip");
    let input_path = dir.join("input.json");
    fs::write(&input_path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();

    let report = decoder::decode_file(&input_path, &dir).expect("decode");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("Compose_Cleaned")),
        "Compose_Cleaned should decode natively, but the decoder reported: {:?}",
        report.warnings
    );

    let pax = fs::read_to_string(dir.join("input.pax")).unwrap();
    assert!(
        pax.contains("let Cleaned = replace(raw, decodeUriComponent(")
            && pax.contains("encodeUriComponent("),
        "expected a native let binding over the URI-component calls, got:\n{pax}"
    );

    // The expression must survive re-emit unchanged, not merely decode.
    let reemitted = compile_pax_to_definition(&dir.join("input.pax"));
    assert_eq!(
        reemitted["definition"]["actions"]["Compose_Cleaned"]["inputs"],
        json!(expr),
        "re-emitted expression drifted from the original"
    );
}

/// The function registry is the gate the decoder consults before rendering a
/// generic call, so a name missing from it silently downgrades a whole action
/// to a `pa` block -- which is how `union` was hiding an entire foreach body.
/// This walks one function from each category the registry covers and asserts
/// the expression round-trips byte-identically, so a future edit that drops or
/// mistypes an entry fails here rather than quietly losing fidelity.
#[test]
fn registry_functions_round_trip_across_categories() {
    use serde_json::json;

    let cases = [
        ("Union", "@union(variables('xs'), variables('xs'))"),
        ("Base64", "@base64ToString(base64('hello'))"),
        ("DateTime", "@addDays(utcNow(), 3, 'yyyy-MM-dd')"),
        ("Json", "@setProperty(json('{}'), 'k', 'v')"),
        ("Uri", "@uriHost('https://example.com/a/b')"),
        ("Math", "@pow(2, 8)"),
        ("Xml", "@xpath(xml('<r><a>1</a></r>'), '//a')"),
        ("Conversion", "@float(string(decimal('1.5')))"),
    ];

    let mut actions = serde_json::Map::new();
    actions.insert(
        "Initialize_variable_xs".to_string(),
        json!({
            "type": "InitializeVariable",
            "runAfter": {},
            "inputs": { "variables": [ { "name": "xs", "type": "array", "value": [] } ] }
        }),
    );
    let mut prev = "Initialize_variable_xs".to_string();
    for (label, expr) in &cases {
        let key = format!("Compose_{label}");
        actions.insert(
            key.clone(),
            json!({
                "type": "Compose",
                "runAfter": { prev.clone(): ["Succeeded"] },
                "inputs": expr
            }),
        );
        prev = key;
    }

    let input = json!({
        "properties": {
            "displayName": "Registry Coverage Round-Trip",
            "definition": {
                "$schema": "https://schema.management.azure.com/providers/Microsoft.Logic/schemas/2016-06-01/workflowdefinition.json#",
                "contentVersion": "1.0.0.0",
                "triggers": { "manual": { "type": "Request", "kind": "Button", "inputs": {} } },
                "actions": actions
            }
        }
    });

    let dir = tmp_dir("registry_categories_roundtrip");
    let input_path = dir.join("input.json");
    fs::write(&input_path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();

    let report = decoder::decode_file(&input_path, &dir).expect("decode");
    assert!(
        report.warnings.is_empty(),
        "every case should decode natively, but the decoder fell back: {:?}",
        report.warnings
    );

    let reemitted = compile_pax_to_definition(&dir.join("input.pax"));
    let emitted = reemitted["definition"]["actions"]
        .as_object()
        .expect("actions object");
    let drifted: Vec<String> = cases
        .iter()
        .filter(|(label, expr)| emitted[&format!("Compose_{label}")]["inputs"] != json!(expr))
        .map(|(label, expr)| {
            format!(
                "{label}: expected {expr:?}, got {:?}",
                emitted[&format!("Compose_{label}")]["inputs"]
            )
        })
        .collect();
    assert!(
        drifted.is_empty(),
        "expressions drifted:\n{}",
        drifted.join("\n")
    );
}

/// PA resolves function names without regard to case, so paxc's registry does
/// too. The risk in matching loosely is that the decoder starts rendering the
/// name as the registry spells it rather than as the author wrote it, which
/// would rewrite a working expression on the way through. Every spelling must
/// survive a round trip exactly as it arrived.
#[test]
fn function_name_casing_survives_the_round_trip() {
    use serde_json::json;

    let cases = [
        ("Canonical", "@toLower('AB')"),
        ("AllLower", "@tolower('AB')"),
        ("AllUpper", "@TOLOWER('AB')"),
        ("OtherFn", "@TOUPPER('ab')"),
        ("Mixed", "@ToLoWeR('AB')"),
    ];

    let mut actions = serde_json::Map::new();
    let mut prev: Option<String> = None;
    for (label, expr) in &cases {
        let key = format!("Compose_{label}");
        let run_after = match &prev {
            Some(p) => json!({ p.clone(): ["Succeeded"] }),
            None => json!({}),
        };
        actions.insert(
            key.clone(),
            json!({ "type": "Compose", "runAfter": run_after, "inputs": expr }),
        );
        prev = Some(key);
    }

    let input = json!({
        "properties": {
            "displayName": "Function Name Casing",
            "definition": {
                "$schema": "https://schema.management.azure.com/providers/Microsoft.Logic/schemas/2016-06-01/workflowdefinition.json#",
                "contentVersion": "1.0.0.0",
                "triggers": { "manual": { "type": "Request", "kind": "Button", "inputs": {} } },
                "actions": actions
            }
        }
    });

    let dir = tmp_dir("function_name_casing");
    let input_path = dir.join("input.json");
    fs::write(&input_path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();

    let report = decoder::decode_file(&input_path, &dir).expect("decode");
    assert!(
        report.warnings.is_empty(),
        "every spelling should resolve, but the decoder fell back: {:?}",
        report.warnings
    );

    let reemitted = compile_pax_to_definition(&dir.join("input.pax"));
    let emitted = reemitted["definition"]["actions"]
        .as_object()
        .expect("actions object");
    let drifted: Vec<String> = cases
        .iter()
        .filter(|(label, expr)| emitted[&format!("Compose_{label}")]["inputs"] != json!(expr))
        .map(|(label, expr)| {
            format!(
                "{label}: expected {expr:?}, got {:?}",
                emitted[&format!("Compose_{label}")]["inputs"]
            )
        })
        .collect();
    assert!(
        drifted.is_empty(),
        "the author's casing must be preserved, not normalized to the \
         registry's spelling:\n{}",
        drifted.join("\n")
    );
}
