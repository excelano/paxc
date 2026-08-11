//! Checks that run against a Power Automate `definition.json` directly,
//! without going through pax.
//!
//! paxc's resolver validates pax source. That leaves the majority of a real
//! flow unexamined, because connector calls, their parameters, and most of
//! the expressions live in `pa/` blocks that paxc carries verbatim and never
//! looks inside. A flow can also arrive here having never been near pax at
//! all — exported from the designer, hand-edited, assembled by an agent.
//! This module works on the artifact PA itself consumes, so it applies to
//! all three.
//!
//! Findings are not `diagnostic::Diagnostic`. That type is built around byte
//! spans into pax source text, and there is no pax source here. A finding
//! carries a JSON path instead.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

pub mod expressions;
pub mod locate;
pub mod runafter;

/// Whether a finding describes a flow that is broken or one that is
/// suspicious. Errors are things PA will import and then silently fail to
/// honor; warnings are shapes that are legal but almost certainly not what
/// the author meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// One problem found in a flow definition.
///
/// `path` locates it the way the JSON nests, so it can be followed by hand
/// in an editor: `actions/Scope/actions/Get_attachments` for a whole action,
/// or `actions/Send_an_email/inputs/parameters/emailMessage/Body` for one
/// field inside one. `code` is stable and machine-greppable; `message` is
/// for a human and `note` carries the reason it matters when that isn't
/// self-evident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub code: &'static str,
    pub path: String,
    pub message: String,
    pub note: Option<String>,
}

impl Finding {
    pub fn error(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            path: path.into(),
            message: message.into(),
            note: None,
        }
    }

    pub fn warning(
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            path: path.into(),
            message: message.into(),
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Sort key, so a run over the same file always reports in the same
    /// order and two runs can be diffed.
    fn sort_key(&self) -> (std::cmp::Reverse<Severity>, &str, &str) {
        (std::cmp::Reverse(self.severity), &self.path, self.code)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: [{}] {}: {}",
            self.severity, self.code, self.path, self.message
        )?;
        if let Some(note) = &self.note {
            write!(f, "\n    note: {note}")?;
        }
        Ok(())
    }
}

/// Why a flow could not be checked at all, as distinct from a flow that was
/// checked and found wanting.
#[derive(Debug)]
pub enum CheckError {
    BadShape(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::BadShape(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CheckError {}

/// Run every check against a flow and return the findings, most severe
/// first.
///
/// `input` may be a full export envelope (`{properties: {definition: ...}}`),
/// the inner properties map, or a bare definition object. A checker gets
/// pointed at whatever file the user has to hand, so all three are accepted
/// rather than making them guess which layer is wanted.
pub fn check_flow(input: &Value) -> Result<Vec<Finding>, CheckError> {
    let definition = locate_definition(input)?;
    let actions = definition
        .get("actions")
        .and_then(Value::as_object)
        .expect("locate_definition only returns a map that has one");

    let mut findings = runafter::check(actions);
    findings.extend(expressions::check(definition));
    findings.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    // One expression can name the same missing thing twice --
    // `union(variables('x'), variables('x'))` is real corpus shape -- and
    // reporting it twice at one path reads as a bug in the checker rather
    // than two problems to fix.
    findings.dedup_by(|a, b| a.code == b.code && a.path == b.path && a.message == b.message);
    Ok(findings)
}

/// Keep the findings that landed inside an opaque `pa/` body and rewrite their
/// paths to name that file, dropping the rest.
///
/// `sources` is `emitter::pa_source_map` — every opaque action's emit path
/// paired with the file its body was read from. A finding matches the longest
/// key that prefixes its path, and what remains is a pointer within that file:
/// `actions/Scope/actions/Send/inputs/to` against a key of
/// `actions/Scope/actions/Send` reports as `pa/Send.json/inputs/to`.
///
/// Findings that match nothing are dropped rather than reported. Those are
/// against JSON paxc generated from pax source the resolver already validated,
/// so one firing means a paxc bug rather than something the author can fix —
/// telling them to go and edit output they never wrote would be worse than
/// silence. `emitted_pax_has_nothing_to_report` is what watches for that case.
///
/// Severity is left alone. Whether these read as warnings or as errors is the
/// caller's policy, not this function's.
pub fn attribute_to_sources(
    findings: Vec<Finding>,
    sources: &BTreeMap<String, PathBuf>,
    relative_to: Option<&Path>,
) -> Vec<Attributed> {
    findings
        .into_iter()
        .filter_map(|mut finding| {
            let (key, source) = sources
                .iter()
                .filter(|(key, _)| {
                    finding.path == **key || finding.path.starts_with(&format!("{key}/"))
                })
                .max_by_key(|(key, _)| key.len())?;
            let shown = relative_to
                .and_then(|base| source.strip_prefix(base).ok())
                .unwrap_or(source.as_path())
                .display()
                .to_string();
            let pointer = finding.path[key.len()..]
                .trim_start_matches('/')
                .to_string();
            finding.path = if pointer.is_empty() {
                shown.clone()
            } else {
                format!("{shown}/{pointer}")
            };
            Some(Attributed {
                finding,
                source: source.clone(),
                display: shown,
                pointer,
            })
        })
        .collect()
}

/// A finding that has been traced back to the `pa/` file it came from.
///
/// The parts are kept apart rather than pre-joined because they are wanted
/// separately: `source` to read the file, `pointer` to find the span inside it,
/// `display` to name it. `finding.path` is the three of them joined for the
/// case where none of that is available and the finding prints as text.
#[derive(Debug, Clone)]
pub struct Attributed {
    pub finding: Finding,
    /// The file on disk, as the resolver read it.
    pub source: PathBuf,
    /// That file as it should be shown — relative to the source directory when
    /// it sits under one, so a finding reads `pa/Send.json` rather than an
    /// absolute path meaningful only on this machine.
    pub display: String,
    /// Where inside the file, `/`-separated, ready for `locate::locate`. Empty
    /// when the finding is about the action as a whole.
    pub pointer: String,
}

/// Peel whichever wrappers are present and return the definition object —
/// the level holding `actions`, and also `triggers` and `parameters`, both
/// of which the expression checks resolve references against.
fn locate_definition(input: &Value) -> Result<&Map<String, Value>, CheckError> {
    let obj = input
        .as_object()
        .ok_or_else(|| CheckError::BadShape("input JSON is not an object".to_string()))?;

    let definition = obj
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|p| p.get("definition"))
        .or_else(|| obj.get("definition"))
        .and_then(Value::as_object)
        .unwrap_or(obj);

    if definition
        .get("actions")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err(CheckError::BadShape(
            "no `actions` map found — expected an export envelope, a properties map, or a \
             definition object"
                .to_string(),
        ));
    }
    Ok(definition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one_action() -> Value {
        json!({"actions": {"A": {"type": "Compose", "runAfter": {}}}})
    }

    #[test]
    fn accepts_bare_definition() {
        assert!(check_flow(&one_action()).is_ok());
    }

    fn sources() -> BTreeMap<String, PathBuf> {
        BTreeMap::from([
            (
                "actions/Send".to_string(),
                PathBuf::from("/src/pa/Send.json"),
            ),
            (
                "actions/Scope/actions/Inner".to_string(),
                PathBuf::from("/src/pa/Inner.json"),
            ),
        ])
    }

    fn at(path: &str) -> Finding {
        Finding::error("x-code", path, "message")
    }

    #[test]
    fn a_finding_inside_a_pa_body_names_the_file_and_keeps_the_rest_as_a_pointer() {
        let out = attribute_to_sources(
            vec![at("actions/Send/inputs/parameters/to")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].finding.path, "pa/Send.json/inputs/parameters/to");
    }

    /// The joined path is for printing. The span work needs the file and the
    /// pointer apart -- one to read, the other to find the line -- and a
    /// pointer that kept its leading slash or the filename would locate
    /// nothing.
    #[test]
    fn the_file_and_the_pointer_are_kept_separable() {
        let out = attribute_to_sources(
            vec![at("actions/Send/inputs/parameters/to")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert_eq!(out[0].source, PathBuf::from("/src/pa/Send.json"));
        assert_eq!(out[0].display, "pa/Send.json");
        assert_eq!(out[0].pointer, "inputs/parameters/to");
    }

    /// A finding about the action as a whole has no field to point at, and an
    /// empty pointer is what tells the caller to underline nothing rather than
    /// guess a line.
    #[test]
    fn an_action_level_finding_has_an_empty_pointer() {
        let out = attribute_to_sources(
            vec![at("actions/Send")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert_eq!(out[0].pointer, "");
    }

    /// Without a source directory to measure against, the absolute path is all
    /// there is -- worse to read, but never wrong.
    #[test]
    fn with_no_base_the_file_shows_as_it_is() {
        let out = attribute_to_sources(vec![at("actions/Send")], &sources(), None);
        assert_eq!(out[0].display, "/src/pa/Send.json");
    }

    #[test]
    fn a_finding_on_the_action_itself_is_just_the_file() {
        let out = attribute_to_sources(
            vec![at("actions/Send")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert_eq!(out[0].finding.path, "pa/Send.json");
    }

    /// A `pa` body can hold actions of its own, so two keys can prefix the same
    /// finding. The deeper one owns it -- picking the shorter would blame the
    /// enclosing file for something in a nested one.
    #[test]
    fn the_longest_matching_key_wins() {
        let mut s = sources();
        s.insert(
            "actions/Scope".to_string(),
            PathBuf::from("/src/pa/Scope.json"),
        );
        let out = attribute_to_sources(
            vec![at("actions/Scope/actions/Inner/inputs")],
            &s,
            Some(Path::new("/src")),
        );
        assert_eq!(out[0].finding.path, "pa/Inner.json/inputs");
    }

    /// `actions/Send` must not claim `actions/Send_an_email`. Matching on the
    /// raw string prefix without requiring a separator would do exactly that,
    /// and PA action names sharing a prefix is completely ordinary.
    #[test]
    fn a_partial_segment_is_not_a_match() {
        let out = attribute_to_sources(
            vec![at("actions/Send_an_email/inputs")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert!(
            out.is_empty(),
            "`actions/Send` must not swallow `actions/Send_an_email`: {out:?}"
        );
    }

    /// Everything paxc emitted from pax source is the resolver's business, and
    /// reporting it would point the author at generated JSON they never wrote.
    #[test]
    fn a_finding_outside_every_pa_body_is_dropped() {
        let out = attribute_to_sources(
            vec![at("actions/Compose_greeting/inputs")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn severity_is_the_callers_business_not_this_functions() {
        let out = attribute_to_sources(
            vec![at("actions/Send")],
            &sources(),
            Some(Path::new("/src")),
        );
        assert_eq!(
            out[0].finding.severity,
            Severity::Error,
            "attribution must not quietly re-rank a finding"
        );
    }

    #[test]
    fn accepts_properties_map() {
        let v = json!({"definition": one_action()});
        assert!(check_flow(&v).is_ok());
    }

    #[test]
    fn accepts_full_envelope() {
        let v = json!({"name": "x", "properties": {"definition": one_action()}});
        assert!(check_flow(&v).is_ok());
    }

    #[test]
    fn rejects_input_without_actions() {
        let v = json!({"properties": {"definition": {"triggers": {}}}});
        assert!(check_flow(&v).is_err());
    }

    #[test]
    fn errors_sort_before_warnings() {
        let mut fs = [
            Finding::warning("w", "actions/B", "later"),
            Finding::error("e", "actions/A", "first"),
        ];
        fs.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        assert_eq!(fs[0].severity, Severity::Error);
    }
}
