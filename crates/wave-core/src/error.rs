use std::fmt;

/// Everything that can go wrong while reading the system.
///
/// Reads are expected to fail routinely — a registry key that another process
/// deleted mid-enumeration, a path the current user may not read. Callers
/// generally log and carry on rather than abort the whole scan, so the
/// variants stay coarse.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{subject} was not found")]
    NotFound { subject: String },

    #[error("access to {subject} was denied")]
    AccessDenied { subject: String },

    #[error("reading {subject} failed: {source}")]
    System {
        subject: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("{0}")]
    Unsupported(String),
}

impl Error {
    pub fn not_found(subject: impl fmt::Display) -> Self {
        Self::NotFound { subject: subject.to_string() }
    }

    pub fn access_denied(subject: impl fmt::Display) -> Self {
        Self::AccessDenied { subject: subject.to_string() }
    }

    pub fn system(
        subject: impl fmt::Display,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::System { subject: subject.to_string(), source: Box::new(source) }
    }

    /// A missing key is the normal case when probing paths that only exist on
    /// some machines, so callers need to tell it apart from a real failure.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

pub type Result<T> = std::result::Result<T, Error>;
