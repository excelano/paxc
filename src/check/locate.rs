//! Find where a path lands in the raw text of a JSON file.
//!
//! A finding against a `pa/` body knows which field it is about —
//! `inputs/parameters/emailMessage/Body` — but not where that field sits in
//! the file, because `serde_json` discards positions as it parses. Without
//! them a finding can name a file and nothing more, which for a connector body
//! several hundred lines deep is barely better than not knowing.
//!
//! So this walks the bytes instead of the parsed value, tracking where it is,
//! and hands back the byte range of the value the path names. That range is
//! what lets a `pa/` finding render through the same ariadne path as a pax
//! compile error, pointing at the line the author actually wrote.
//!
//! It is a locator, not a parser, and deliberately not a second implementation
//! of JSON. It never validates: the file has already been through `serde_json`
//! by the time anything here runs, so malformed input is not a case that
//! arises, and every failure to find a path returns `None` and costs the caller
//! a span rather than a compile.

use std::ops::Range;

/// Byte range of the value at `path` within `src`, where `path` is
/// `/`-separated and indexes objects by key and arrays by number:
/// `inputs/parameters/To/0`. An empty path is the whole document.
///
/// Returns `None` when the path names nothing, which is not an error — a
/// finding whose field was synthesized rather than written (PA defaults an
/// absent `runAfter`, for one) has no text to point at, and the caller falls
/// back to naming the file.
///
/// A key containing a literal `/` cannot be addressed, since the separator is
/// the separator. No PA action name or connector parameter key observed in the
/// corpus contains one, and the cost of guessing wrong is a missing span.
pub fn locate(src: &str, path: &str) -> Option<Range<usize>> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    descend(src, 0, &segments)
}

fn descend(src: &str, from: usize, segments: &[&str]) -> Option<Range<usize>> {
    let b = src.as_bytes();
    let i = skip_ws(b, from);
    if segments.is_empty() {
        return Some(i..skip_value(src, i)?);
    }
    match b.get(i)? {
        b'{' => descend_object(src, i + 1, segments),
        b'[' => descend_array(src, i + 1, segments),
        // The path goes deeper than the document does.
        _ => None,
    }
}

fn descend_object(src: &str, from: usize, segments: &[&str]) -> Option<Range<usize>> {
    let b = src.as_bytes();
    let mut i = from;
    loop {
        i = skip_ws(b, i);
        if *b.get(i)? != b'"' {
            // A closing brace: the object ran out before the key turned up.
            return None;
        }
        let key_end = skip_string(b, i)?;
        // Decoding through serde_json rather than by hand so an escaped key
        // compares as what it means, not as how it is spelled.
        let key: String = serde_json::from_str(&src[i..key_end]).ok()?;
        i = skip_ws(b, key_end);
        if *b.get(i)? != b':' {
            return None;
        }
        i = skip_ws(b, i + 1);
        if key == segments[0] {
            return descend(src, i, &segments[1..]);
        }
        i = skip_ws(b, skip_value(src, i)?);
        match b.get(i)? {
            b',' => i += 1,
            _ => return None,
        }
    }
}

fn descend_array(src: &str, from: usize, segments: &[&str]) -> Option<Range<usize>> {
    let b = src.as_bytes();
    let want: usize = segments[0].parse().ok()?;
    let mut i = from;
    let mut index = 0usize;
    loop {
        i = skip_ws(b, i);
        if *b.get(i)? == b']' {
            return None;
        }
        if index == want {
            return descend(src, i, &segments[1..]);
        }
        i = skip_ws(b, skip_value(src, i)?);
        match b.get(i)? {
            b',' => {
                i += 1;
                index += 1;
            }
            _ => return None,
        }
    }
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Index just past the value starting at `i`.
fn skip_value(src: &str, i: usize) -> Option<usize> {
    let b = src.as_bytes();
    let i = skip_ws(b, i);
    match b.get(i)? {
        b'"' => skip_string(b, i),
        b'{' => skip_container(b, i, b'{', b'}'),
        b'[' => skip_container(b, i, b'[', b']'),
        // A number, `true`, `false` or `null` — everything up to whatever
        // delimits it. The file has already parsed, so it is one of those.
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// Index just past the closing quote of the string starting at `i`.
fn skip_string(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            // Whatever follows a backslash is consumed without being read, so
            // an escaped quote cannot end the string early. `\uXXXX` needs no
            // special case: its four hex digits are ordinary characters.
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// Index just past the container starting at `i`.
///
/// Only the container's own delimiters are counted, so a `[` nested inside an
/// object needs no handling — it cannot close a brace. Strings are stepped
/// over whole, which is what keeps a `}` inside one from ending the object.
fn skip_container(b: &[u8], i: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        let c = b[j];
        if c == b'"' {
            j = skip_string(b, j)?;
        } else if c == open {
            depth += 1;
            j += 1;
        } else if c == close {
            depth -= 1;
            j += 1;
            if depth == 0 {
                return Some(j);
            }
        } else {
            j += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{
  "type": "OpenApiConnection",
  "inputs": {
    "host": { "operationId": "SendEmailV2" },
    "parameters": {
      "emailMessage/Body": "unaddressable",
      "To": ["a@example.com", "b@example.com"],
      "Subject": "@concat('a', 'b')"
    },
    "retry": { "count": 4, "enabled": true }
  }
}"#;

    fn text(path: &str) -> &str {
        let range = locate(DOC, path).expect("path should resolve");
        &DOC[range]
    }

    #[test]
    fn finds_a_nested_object_field() {
        assert_eq!(text("inputs/parameters/Subject"), "\"@concat('a', 'b')\"");
    }

    #[test]
    fn finds_an_array_element_by_index() {
        assert_eq!(text("inputs/parameters/To/1"), "\"b@example.com\"");
    }

    #[test]
    fn finds_a_whole_container() {
        assert_eq!(text("inputs/host"), "{ \"operationId\": \"SendEmailV2\" }");
    }

    #[test]
    fn finds_non_string_scalars() {
        assert_eq!(text("inputs/retry/count"), "4");
        assert_eq!(text("inputs/retry/enabled"), "true");
    }

    #[test]
    fn an_empty_path_is_the_whole_document() {
        assert_eq!(locate(DOC, ""), Some(0..DOC.len()));
    }

    /// The span has to be the value, not the key and not the whole member, or
    /// the caret lands one field to the left of the problem.
    #[test]
    fn the_span_starts_at_the_value_not_the_key() {
        let range = locate(DOC, "inputs/parameters/Subject").unwrap();
        assert_eq!(DOC.as_bytes()[range.start], b'"');
        assert_eq!(&DOC[range.start..range.start + 2], "\"@");
    }

    #[test]
    fn a_missing_key_is_none_rather_than_a_guess() {
        assert_eq!(locate(DOC, "inputs/parameters/Cc"), None);
        assert_eq!(locate(DOC, "inputs/nope/deeper"), None);
        assert_eq!(locate(DOC, "inputs/parameters/To/9"), None);
    }

    /// Keys sharing a prefix must not match each other, and a key that is a
    /// prefix of the one wanted must not stop the scan early.
    #[test]
    fn a_key_is_matched_whole() {
        let doc = r#"{"Send": 1, "Send_an_email": 2, "Sen": 3}"#;
        assert_eq!(&doc[locate(doc, "Send").unwrap()], "1");
        assert_eq!(&doc[locate(doc, "Send_an_email").unwrap()], "2");
        assert_eq!(&doc[locate(doc, "Sen").unwrap()], "3");
    }

    /// A brace or bracket inside a string must not be read as structure. This
    /// is the ordinary case in PA, not a corner one: expressions are strings
    /// and they are full of both.
    #[test]
    fn punctuation_inside_a_string_is_not_structure() {
        let doc = r#"{"a": "if(x, '}', ']')", "b": "{[", "c": 7}"#;
        assert_eq!(&doc[locate(doc, "c").unwrap()], "7");
        assert_eq!(&doc[locate(doc, "b").unwrap()], "\"{[\"");
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        let doc = r#"{"a": "she said \"no\", loudly", "b": 2}"#;
        assert_eq!(&doc[locate(doc, "b").unwrap()], "2");
    }

    /// An escaped key compares as what it means rather than how it is written,
    /// which is why the comparison goes through serde_json.
    #[test]
    fn an_escaped_key_matches_its_decoded_form() {
        let doc = r#"{"a\nb": 1, "plain": 2}"#;
        assert_eq!(&doc[locate(doc, "a\nb").unwrap()], "1");
    }

    /// Documenting the one addressing limit rather than pretending it away:
    /// `/` is the separator, so a key holding one cannot be reached.
    #[test]
    fn a_key_containing_a_slash_cannot_be_addressed() {
        assert_eq!(locate(DOC, "inputs/parameters/emailMessage/Body"), None);
    }

    /// Offsets are byte offsets, which is what the renderer expects and
    /// translates from. Multi-byte text earlier in the file must not shift
    /// them, or the caret drifts left by one column per umlaut -- and PA
    /// bodies carry German, French and quotation dashes as a matter of course.
    #[test]
    fn multi_byte_text_does_not_shift_the_offsets() {
        let doc = r#"{"gruss": "Schöne Grüße — München", "broken": "@x()"}"#;
        let range = locate(doc, "broken").expect("resolves");
        assert_eq!(&doc[range.clone()], "\"@x()\"");
        // Slicing at these offsets is only valid if they are byte offsets on
        // char boundaries; a char-offset range would panic or cut a codepoint.
        assert!(doc.is_char_boundary(range.start));
        assert!(doc.is_char_boundary(range.end));
    }

    #[test]
    fn deeply_nested_containers_do_not_confuse_the_scan() {
        let doc = r#"{"a": {"b": [{"c": [[1, 2], {"d": "x"}]}]}, "z": 9}"#;
        assert_eq!(&doc[locate(doc, "a/b/0/c/1/d").unwrap()], "\"x\"");
        assert_eq!(&doc[locate(doc, "z").unwrap()], "9");
    }
}
