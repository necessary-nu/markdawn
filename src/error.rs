use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    AlreadyFinished,
    InvalidExtensionMatch,
    MalformedEventStream,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyFinished => f.write_str("parser has already been finished"),
            Self::InvalidExtensionMatch => f.write_str("extension returned an invalid match"),
            Self::MalformedEventStream => {
                f.write_str("markdown parser produced a malformed event stream")
            }
        }
    }
}

impl std::error::Error for Error {}

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
