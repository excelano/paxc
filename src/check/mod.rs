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

use std::fmt;

use serde_json::{Map, Value};

pub mod expressions;
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
