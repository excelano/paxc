//! The `--check` pass, run against real exported flows.
//!
//! Two things are worth proving on real data that unit tests on synthetic
//! JSON cannot. First, that six flows exported from a live tenant produce no
//! findings at all: a checker that cries wolf on working flows is worse than
//! none, because it teaches its reader to skip the output. Second, that
//! breaking one of those flows in the ways a person actually breaks them is
//! caught, on the real nesting rather than on a two-action toy.

use std::fs;
use std::path::{Path, PathBuf};

use paxc::check::{self, Severity, expressions, runafter};
use serde_json::{Map, Value};

/// See the note on the same constant in `tests/decoder.rs`. CI runs without
/// the corpus on purpose.
const ALLOW_MISSING_CORPUS: &str = "PAXC_ALLOW_MISSING_CORPUS";

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

/// Returns None and prints a skip line when the corpus is absent and the
/// caller has said that is allowed; panics when it is absent and nobody
/// said so.
fn corpus_or_skip(test: &str) -> Option<Vec<PathBuf>> {
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
            "skipping {test}: no corpus at {} and {ALLOW_MISSING_CORPUS} is set",
            root.display()
        );
        return None;
    }
    let entries = corpus_entries();
    assert!(
        !entries.is_empty(),
        "corpus at {} holds no <name>/input.json",
        root.display()
    );
    Some(entries)
}

fn load(entry: &Path) -> Value {
    let bytes = fs::read(entry.join("input.json")).expect("read corpus input");
    serde_json::from_slice(&bytes).expect("parse corpus input")
}

fn label(entry: &Path) -> String {
    entry.file_name().unwrap().to_string_lossy().to_string()
}

fn actions_mut(flow: &mut Value) -> &mut Map<String, Value> {
    flow.get_mut("properties")
        .and_then(|p| p.get_mut("definition"))
        .and_then(|d| d.get_mut("actions"))
        .and_then(Value::as_object_mut)
        .expect("corpus flow has properties.definition.actions")
}

/// Name of the first top-level action that waits on something, paired with
/// the first thing it waits on.
fn first_edge(flow: &Value) -> Option<(String, String)> {
    let actions = flow
        .get("properties")?
        .get("definition")?
        .get("actions")?
        .as_object()?;
    for (name, action) in actions {
        // `?` here would abandon the whole search the moment one action
        // lacked a runAfter, rather than moving on to the next.
        let run_after = action.get("runAfter").and_then(Value::as_object);
        if let Some(target) = run_after.and_then(|m| m.keys().next()) {
            return Some((name.clone(), target.clone()));
        }
    }
    None
}

fn codes(findings: &[check::Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.code).collect()
}

#[test]
fn corpus_flows_check_clean() {
    let Some(entries) = corpus_or_skip("corpus_flows_check_clean") else {
        return;
    };

    let mut noisy: Vec<String> = Vec::new();
    for entry in &entries {
        let findings = check::check_flow(&load(entry)).expect("corpus flow has a checkable shape");
        if !findings.is_empty() {
            let rendered: Vec<String> = findings.iter().map(|f| f.to_string()).collect();
            noisy.push(format!("{}:\n{}", label(entry), rendered.join("\n")));
        }
    }

    assert!(
        noisy.is_empty(),
        "flows exported from a working tenant produced findings, which means a \
         false positive rather than a broken flow:\n{}",
        noisy.join("\n\n")
    );
}

#[test]
fn renaming_an_action_leaves_a_dangling_edge() {
    let Some(entries) = corpus_or_skip("renaming_an_action_leaves_a_dangling_edge") else {
        return;
    };

    let mut mutated = 0;
    for entry in &entries {
        let mut flow = load(entry);
        let Some((waiter, target)) = first_edge(&flow) else {
            continue;
        };
        // The rename a designer does: the action gets a new key, and every
        // back-reference to the old one is left behind.
        let actions = actions_mut(&mut flow);
        let Some(body) = actions.remove(&target) else {
            continue;
        };
        actions.insert(format!("{target}_renamed"), body);

        let findings = check::check_flow(&flow).expect("still a checkable shape");
        assert!(
            codes(&findings).contains(&runafter::UNKNOWN_TARGET),
            "{}: renaming `{target}` left `{waiter}` pointing at nothing and the check \
             did not report it. Findings: {:?}",
            label(entry),
            codes(&findings)
        );
        mutated += 1;
    }

    assert!(
        mutated > 0,
        "no corpus flow had a top-level runAfter edge to break"
    );
}

#[test]
fn a_misspelled_status_is_caught() {
    let Some(entries) = corpus_or_skip("a_misspelled_status_is_caught") else {
        return;
    };

    let mut mutated = 0;
    for entry in &entries {
        let mut flow = load(entry);
        let Some((waiter, target)) = first_edge(&flow) else {
            continue;
        };
        actions_mut(&mut flow)[&waiter]["runAfter"][&target] = serde_json::json!(["Suceeded"]);

        let findings = check::check_flow(&flow).expect("still a checkable shape");
        assert!(
            codes(&findings).contains(&runafter::BAD_STATUS),
            "{}: `Suceeded` went unreported",
            label(entry)
        );
        mutated += 1;
    }
    assert!(mutated > 0);
}

#[test]
fn an_emptied_status_list_is_a_warning_not_an_error() {
    let Some(entries) = corpus_or_skip("an_emptied_status_list_is_a_warning_not_an_error") else {
        return;
    };

    let mut mutated = 0;
    for entry in &entries {
        let mut flow = load(entry);
        let Some((waiter, target)) = first_edge(&flow) else {
            continue;
        };
        actions_mut(&mut flow)[&waiter]["runAfter"][&target] = serde_json::json!([]);

        let findings = check::check_flow(&flow).expect("still a checkable shape");
        let empty: Vec<&check::Finding> = findings
            .iter()
            .filter(|f| f.code == runafter::EMPTY_STATUS)
            .collect();
        assert_eq!(
            empty.len(),
            1,
            "{}: expected one empty-status finding",
            label(entry)
        );
        assert_eq!(empty[0].severity, Severity::Warning);
        mutated += 1;
    }
    assert!(mutated > 0);
}

#[test]
fn waiting_across_a_scope_boundary_is_caught() {
    let Some(entries) = corpus_or_skip("waiting_across_a_scope_boundary_is_caught") else {
        return;
    };

    // Only flows with a nested scope can exercise this, and the corpus is
    // not guaranteed to keep one forever. Count them, and fail if none is
    // left rather than passing on an empty loop.
    let mut mutated = 0;
    for entry in &entries {
        let mut flow = load(entry);
        let Some((container, _)) = first_container(&flow) else {
            continue;
        };

        // Point the nested scope's own first action at the container that
        // holds it — a real name, unreachable from where it is written.
        let actions = actions_mut(&mut flow);
        let nested = actions[&container]["actions"]
            .as_object_mut()
            .expect("container has an actions map");
        let first = nested
            .keys()
            .next()
            .cloned()
            .expect("container is not empty");
        nested[&first]["runAfter"] = serde_json::json!({ container.clone(): ["Succeeded"] });

        let findings = check::check_flow(&flow).expect("still a checkable shape");
        assert!(
            codes(&findings).contains(&runafter::CROSS_SCOPE),
            "{}: `{first}` waiting on its own container went unreported as cross-scope. \
             Findings: {:?}",
            label(entry),
            codes(&findings)
        );
        mutated += 1;
    }

    assert!(
        mutated > 0,
        "no corpus flow has a top-level action with a non-empty nested actions map"
    );
}

#[test]
fn renaming_a_variable_breaks_every_use_of_it() {
    let Some(entries) = corpus_or_skip("renaming_a_variable_breaks_every_use_of_it") else {
        return;
    };

    let mut mutated = 0;
    for entry in &entries {
        let mut flow = load(entry);
        let renamed = rename_all_variables(actions_mut(&mut flow));
        if renamed == 0 {
            continue;
        }

        // Whether the flow uses its variables at all, decided independently
        // of the checker. One corpus flow declares `currentSignedOff` and
        // never reads it, so a rename there is correctly silent and the test
        // must not demand otherwise.
        let text = serde_json::to_string(&load(entry)).expect("reserialize");
        let uses_variables = text.to_lowercase().contains("variables('")
            || ["SetVariable", "IncrementVariable", "DecrementVariable"]
                .iter()
                .chain(["AppendToArrayVariable", "AppendToStringVariable"].iter())
                .any(|t| text.contains(t));
        if !uses_variables {
            continue;
        }

        let findings = check::check_flow(&flow).expect("still a checkable shape");
        let broken = findings
            .iter()
            .filter(|f| f.code == expressions::UNKNOWN_VARIABLE)
            .count();
        assert!(
            broken > 0,
            "{}: renaming all {renamed} variable declarations left every use unreported",
            label(entry)
        );
        mutated += 1;
    }
    assert!(
        mutated > 0,
        "no corpus flow both declares and uses a variable"
    );
}

#[test]
fn a_typo_in_an_action_reference_is_caught() {
    let Some(entries) = corpus_or_skip("a_typo_in_an_action_reference_is_caught") else {
        return;
    };

    // Retarget one accessor call by name, on the serialized flow, so the
    // mutation is the same edit a person makes in a text editor.
    let mut mutated = 0;
    for entry in &entries {
        let text = serde_json::to_string(&load(entry)).expect("reserialize");
        let Some(start) = ["body('", "outputs('", "actions('"]
            .iter()
            .filter_map(|open| text.find(open).map(|i| i + open.len()))
            .min()
        else {
            continue;
        };
        let Some(end) = text[start..].find('\'').map(|i| start + i) else {
            continue;
        };
        let referenced = &text[start..end];
        // A trailing character keeps it a plausible name rather than an
        // obviously empty one.
        let broken = format!("{}{}{}", &text[..end], "_typo", &text[end..]);
        let flow: Value = serde_json::from_str(&broken).expect("still valid JSON");

        let findings = check::check_flow(&flow).expect("still a checkable shape");
        assert!(
            codes(&findings).contains(&expressions::UNKNOWN_ACTION),
            "{}: `{referenced}_typo` resolved to nothing and went unreported. Findings: {:?}",
            label(entry),
            codes(&findings)
        );
        mutated += 1;
    }
    assert!(mutated > 0, "no corpus flow references an action by name");
}

#[test]
fn an_unclosed_paren_in_a_real_flow_is_caught() {
    let Some(entries) = corpus_or_skip("an_unclosed_paren_in_a_real_flow_is_caught") else {
        return;
    };

    for entry in &entries {
        let mut flow = load(entry);
        actions_mut(&mut flow).insert(
            "Compose_unbalanced".to_string(),
            serde_json::json!({
                "type": "Compose",
                "runAfter": {},
                "inputs": "@concat('a', 'b'",
            }),
        );
        let findings = check::check_flow(&flow).expect("still a checkable shape");
        assert!(
            codes(&findings).contains(&expressions::UNBALANCED_PARENS),
            "{}: unclosed paren went unreported",
            label(entry)
        );
    }
}

/// Rename every variable a flow initializes, leaving all references to them
/// behind. Returns how many were renamed.
fn rename_all_variables(actions: &mut Map<String, Value>) -> usize {
    let mut renamed = 0;
    for action in actions.values_mut() {
        let Some(action) = action.as_object_mut() else {
            continue;
        };
        if action.get("type").and_then(Value::as_str) == Some("InitializeVariable") {
            let declared = action
                .get_mut("inputs")
                .and_then(|i| i.get_mut("variables"))
                .and_then(Value::as_array_mut);
            for var in declared.into_iter().flatten() {
                if let Some(name) = var.get("name").and_then(Value::as_str) {
                    var["name"] = Value::String(format!("{name}_renamed"));
                    renamed += 1;
                }
            }
        }
        // Recurse into whichever container shape this action uses.
        for key in ["actions", "else", "default"] {
            let nested = match key {
                "actions" => action.get_mut(key).and_then(Value::as_object_mut),
                _ => action
                    .get_mut(key)
                    .and_then(Value::as_object_mut)
                    .and_then(|b| b.get_mut("actions"))
                    .and_then(Value::as_object_mut),
            };
            if let Some(nested) = nested {
                renamed += rename_all_variables(nested);
            }
        }
    }
    renamed
}

/// First top-level action carrying a non-empty nested `actions` map, with
/// its type.
fn first_container(flow: &Value) -> Option<(String, String)> {
    let actions = flow
        .get("properties")?
        .get("definition")?
        .get("actions")?
        .as_object()?;
    for (name, action) in actions {
        let nested = action.get("actions").and_then(Value::as_object);
        if nested.is_some_and(|n| !n.is_empty()) {
            let ty = action
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            return Some((name.clone(), ty));
        }
    }
    None
}
