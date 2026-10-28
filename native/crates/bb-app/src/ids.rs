//! Opaque ids. They are never reused within a run, so a stale one is detectable (`AppError::Stale`).

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
        pub struct $name(pub u64);
    };
}

id!(PaneId);
id!(TabId);
id!(OpId);

/// What a call into the app can fail with. The FFI maps `Stale` to `BB_STALE` and the rest to `BB_ERR`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    /// An id named something that no longer exists.
    Stale,
    /// The request was malformed or refused; the message is for the user or the log.
    Invalid(String),
    /// Another operation holds the app.
    Busy,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Stale => write!(f, "stale id"),
            AppError::Invalid(m) => write!(f, "{m}"),
            AppError::Busy => write!(f, "busy: another operation is running"),
        }
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;
