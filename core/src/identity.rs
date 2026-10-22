//! Validated internal identities. Platform JSON continues to carry strings.
use crate::Result;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct VideoId(String);

impl VideoId {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value.len() != 11
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            return Err("Invalid video identity.".into());
        }
        Ok(Self(value.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TrackId(String);

impl TrackId {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        // Empty track IDs are accepted by the existing cache protocol.
        if value.len() > 1_000 {
            return Err("Invalid caption track.".into());
        }
        Ok(Self(value.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SessionId(String);

impl SessionId {
    pub(crate) fn parse(value: &str, max_bytes: usize) -> Result<Self> {
        if value.is_empty()
            || value.len() > max_bytes
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_:-".contains(&byte))
        {
            return Err("Invalid session identity.".into());
        }
        Ok(Self(value.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
