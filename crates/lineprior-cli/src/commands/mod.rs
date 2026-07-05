pub mod build;
pub mod query;
pub mod summary;
pub mod validate;

use std::process::ExitCode;

/// Maps a lineprior parse/build error to its documented exit code:
/// `NoObservations` is "no usable data" (2); a bare I/O fault reading an
/// already-opened stream is an internal error (4); everything else is bad
/// input (invalid JSON, missing/invalid fields) (3).
pub fn exit_code_for_lineprior_error(err: &lineprior::Error) -> ExitCode {
    match err {
        lineprior::Error::NoObservations => ExitCode::from(2),
        lineprior::Error::Io(_) => ExitCode::from(4),
        _ => ExitCode::from(3),
    }
}
