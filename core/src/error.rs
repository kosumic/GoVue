//! Internal failures retain their cause; JSON exposes only the stable message.
use serde::{Serialize, Serializer};
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Error {
    message: String,
    operation: Option<&'static str>,
    source: Option<Arc<dyn StdError + Send + Sync>>,
}

impl Error {
    pub(crate) fn caused_by(
        message: impl Into<String>,
        operation: &'static str,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            message: message.into(),
            operation: Some(operation),
            source: Some(Arc::new(source)),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.message
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self {
            message,
            operation: None,
            source: None,
        }
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Error")
            .field("message", &self.message)
            .field("operation", &self.operation)
            .field("source", &self.source)
            .finish()
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
