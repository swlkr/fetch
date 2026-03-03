use crate::Headers;
pub struct ResponseHead {
    pub status: u16,
    pub reason: String,
    pub headers: Headers,
    pub content_length: Option<usize>,
    pub is_chunked: bool,

    pub body_offset: usize,
}
