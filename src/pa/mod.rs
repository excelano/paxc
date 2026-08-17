//! Power Automate target.
//!
//! Everything PA-specific lives under this module: the JSON emitter, the
//! Legacy Import Package zip builder, and (in `names`) the action-type and
//! function-library names that PA hardcodes. Modules outside `pa` should
//! be target-agnostic where reasonable; when they need a PA name, they
//! pull it from here rather than inlining the string.

pub mod decoder;
pub mod emitter;
pub mod functions;
pub mod names;
pub mod packager;
pub(crate) mod paexpr;

/// An error from the zip backend, with the backend's own type kept out of
/// the way.
///
/// Both `packager::PackageError` and `decoder::DecodeError` are public and
/// carry archive failures. Naming the zip crate's error type in those
/// variants would put it in paxc's public API, which makes every major
/// version of that crate a breaking change here. This carries the rendered
/// message instead, which is all either caller ever did with it.
///
/// See `JsonError` for the same treatment of the JSON backend.
#[derive(Debug)]
pub struct ZipError(String);

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ZipError {}

impl ZipError {
    /// Deliberately a crate-internal constructor rather than a `From` impl:
    /// a public `From<zip::result::ZipError>` would name the backend's type
    /// in paxc's API again, which is what this type exists to prevent.
    pub(crate) fn new(source: zip::result::ZipError) -> Self {
        ZipError(source.to_string())
    }
}

/// An error from the JSON backend, kept out of paxc's public API for the
/// same reason as `ZipError`.
///
/// `resolver::ResolveError::PaFileInvalidJson` already rendered its parse
/// failure to a `String` rather than carrying serde_json's type; this brings
/// `packager::PackageError::Json` and `decoder::DecodeError::JsonParse` into
/// line with it.
#[derive(Debug)]
pub struct JsonError(String);

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonError {}

impl JsonError {
    /// Crate-internal for the reason spelled out on `ZipError::new`.
    pub(crate) fn new(source: serde_json::Error) -> Self {
        JsonError(source.to_string())
    }
}
