use std::str::Utf8Error;

#[derive(PartialEq, Debug)]
pub struct Error(String);
impl Error {
    pub fn new(_0: String) -> Self {
        Self(_0)
    }
}
pub fn error(s: impl std::fmt::Display) -> Error {
    Error::new(s.to_string())
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        error(value)
    }
}
impl From<std::num::ParseIntError> for Error {
    fn from(value: std::num::ParseIntError) -> Self {
        error(value)
    }
}
impl From<Utf8Error> for Error {
    fn from(value: Utf8Error) -> Self {
        error(value)
    }
}
