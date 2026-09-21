use crate::metadata::Metadata;
use crate::{reader, writer};
use std::borrow::Cow;
use std::io;
use std::num::ParseIntError;
use std::sync::{PoisonError, RwLockReadGuard, RwLockWriteGuard};
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::task;

#[derive(Debug, Error)]
pub enum Error {
    #[error("DecodeError: {0}")]
    Decode(String),
    #[error("Generic: {0}")]
    Generic(Cow<'static, str>),
    #[error("EncodeError: {0}")]
    Encode(String),
    #[error("FileCorrupted: {0}")]
    FileCorrupted(Cow<'static, str>),
    /// The entry stream for an append ended without its end-of-stream marker, so the writer
    /// received a prefix of a batch whose extent it cannot know. Distinct from an IO failure:
    /// the bytes that were received are on their way to disk, and it is the *batch* that is
    /// incomplete.
    #[error("IncompleteAppend: {0}")]
    IncompleteAppend(Cow<'static, str>),
    #[error("Integrity: {0}")]
    Integrity(Cow<'static, str>),
    #[error("Internal: {0}")]
    Internal(Cow<'static, str>),
    #[error("InvalidPath: {0}")]
    InvalidPath(&'static str),
    #[error("InvalidFileName")]
    InvalidFileName,
    #[error("IOError: {0}")]
    IO(#[from] io::Error),
    #[error("Locked: {0}")]
    Locked(&'static str),
    #[error("ParseError: {0}")]
    Parse(&'static str),
    #[error("WalSizeExceeded: {0}")]
    WalSizeExceeded(Cow<'static, str>),
}

impl Error {
    /// The `io::Error` form of this error, for OpenRaft's log I/O completion callback.
    ///
    /// `LogFlushed::log_io_completed` carries a `Result<(), io::Error>`, while the append
    /// acknowledgement channel carries the typed `Error`. `Error` is not `Clone`, so a failure
    /// that has to reach both places is reproduced here rather than moved. An `Error::IO` keeps
    /// its `io::ErrorKind`; every other variant becomes `ErrorKind::Other` carrying the same
    /// `Display` text, so the cause survives in both directions.
    pub fn as_io_error(&self) -> io::Error {
        match self {
            Error::IO(err) => io::Error::new(err.kind(), err.to_string()),
            other => io::Error::other(other.to_string()),
        }
    }
}

impl From<task::JoinError> for Error {
    fn from(err: task::JoinError) -> Self {
        Self::Generic(err.to_string().into())
    }
}

impl From<bincode::error::DecodeError> for Error {
    fn from(err: bincode::error::DecodeError) -> Self {
        Self::Decode(err.to_string())
    }
}

impl From<bincode::error::EncodeError> for Error {
    fn from(err: bincode::error::EncodeError) -> Self {
        Self::Encode(err.to_string())
    }
}

impl From<ParseIntError> for Error {
    fn from(_: ParseIntError) -> Self {
        Self::Parse("Cannot parse value as integer")
    }
}

impl From<PoisonError<RwLockReadGuard<'_, Metadata>>> for Error {
    fn from(err: PoisonError<RwLockReadGuard<Metadata>>) -> Self {
        Self::Generic(err.to_string().into())
    }
}

impl From<PoisonError<RwLockWriteGuard<'_, Metadata>>> for Error {
    fn from(err: PoisonError<RwLockWriteGuard<Metadata>>) -> Self {
        Self::Generic(err.to_string().into())
    }
}

impl From<flume::SendError<writer::Action>> for Error {
    fn from(err: flume::SendError<writer::Action>) -> Self {
        Self::Internal(err.to_string().into())
    }
}

impl From<flume::SendError<reader::Action>> for Error {
    fn from(err: flume::SendError<reader::Action>) -> Self {
        Self::Internal(err.to_string().into())
    }
}

impl From<oneshot::error::RecvError> for Error {
    fn from(err: oneshot::error::RecvError) -> Self {
        Self::Generic(err.to_string().into())
    }
}
