use core::fmt;

/// Byte offset into the parser's normalized buffer where an issue was
/// detected.
// [spec:mw:sem:core.error.location]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorLocation {
    pub byte_offset: usize,
}

// [spec:mw:def:core.error.taxonomy]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    AlreadyFinished,
    /// An `Extension::try_inline` returned a match that overruns the input or
    /// lands on a non-UTF-8 boundary.
    InvalidExtensionMatch {
        location: Option<ErrorLocation>,
    },
    /// The inline tokenizer reached a state that should be impossible — only
    /// raised when input contains malformed UTF-8 or a parser invariant was
    /// violated.
    MalformedEventStream {
        location: Option<ErrorLocation>,
    },
    /// Cumulative input bytes across `push_str` / `finish` /
    /// `parse_full_at` exceeded the configured cap
    /// (`ParserBuilder::with_max_input_bytes`). Only raised when the
    /// caller has opted in; default is unlimited.
    InputTooLarge {
        seen: usize,
        cap: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyFinished => f.write_str("parser has already been finished"),
            Self::InvalidExtensionMatch { location } => {
                f.write_str("extension returned an invalid match")?;
                if let Some(loc) = location {
                    write!(f, " at byte {}", loc.byte_offset)?;
                }
                Ok(())
            }
            Self::MalformedEventStream { location } => {
                f.write_str("markdown parser produced a malformed event stream")?;
                if let Some(loc) = location {
                    write!(f, " at byte {}", loc.byte_offset)?;
                }
                Ok(())
            }
            Self::InputTooLarge { seen, cap } => {
                write!(
                    f,
                    "markdown input exceeded configured cap: {seen} bytes seen, cap is {cap}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

// [spec:mw:def:core.error.stream-error]
#[derive(Debug)]
pub enum StreamError<E> {
    Parser(Error),
    Sink(E),
}

impl<E> From<Error> for StreamError<E> {
    fn from(value: Error) -> Self {
        Self::Parser(value)
    }
}

impl<E> fmt::Display for StreamError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parser(error) => error.fmt(f),
            Self::Sink(error) => error.fmt(f),
        }
    }
}

impl<E> std::error::Error for StreamError<E> where E: std::error::Error + 'static {}
