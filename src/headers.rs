use crate::Header;
use std::fmt::Display;

pub struct Headers(pub Vec<Header>);

impl Headers {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn push(mut self, name: impl Display, value: impl Display) -> Self {
        self.0.push(Header {
            name: name.to_string(),
            value: value.to_string(),
        });
        self
    }
}

pub fn headers() -> Headers {
    Headers(vec![])
}
