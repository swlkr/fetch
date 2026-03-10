use crate::{Headers, ResponseHead, Result, chunk_iter::ChunkIter, event_iter::EventIter};
use std::io::{BufReader, Read};

pub struct Response<R: Read> {
    pub status: u16,
    pub reason: String,
    pub headers: Headers,
    reader: BufReader<R>,
    content_length: Option<usize>,
    is_chunked: bool,
}

impl<R: Read> Response<R> {
    pub fn from_parts(head: ResponseHead, reader: BufReader<R>) -> Self {
        Response {
            status: head.status,
            reason: head.reason,
            headers: head.headers,
            reader,
            content_length: head.content_length,
            is_chunked: head.is_chunked,
        }
    }

    pub fn body(mut self) -> Result<Vec<u8>> {
        if let Some(len) = self.content_length {
            let mut buf = vec![0u8; len];
            self.reader.read_exact(&mut buf)?;
            Ok(buf)
        } else if self.is_chunked {
            let mut result = Vec::new();
            for chunk in self.chunks() {
                result.extend(chunk?);
            }
            Ok(result)
        } else {
            let mut buf = Vec::new();
            self.reader.read_to_end(&mut buf)?;
            Ok(buf)
        }
    }

    pub fn chunks(&mut self) -> ChunkIter<'_, R> {
        ChunkIter {
            reader: &mut self.reader,
            done: false,
        }
    }

    pub fn events(&mut self) -> EventIter<'_, R> {
        EventIter {
            reader: &mut self.reader,
            is_chunked: self.is_chunked,
            chunk_remaining: 0,
            chunk_done: false,
        }
    }
}
