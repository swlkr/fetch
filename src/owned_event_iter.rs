use crate::{Result, SseEvent, error};
use std::io::{BufRead, BufReader, Read};

pub struct OwnedEventIter<R: Read> {
    reader: BufReader<R>,
    is_chunked: bool,
    chunk_remaining: usize,
    chunk_done: bool,
}

impl<R: Read> OwnedEventIter<R> {
    pub fn new(reader: BufReader<R>, is_chunked: bool) -> Self {
        OwnedEventIter {
            reader,
            is_chunked,
            chunk_remaining: 0,
            chunk_done: false,
        }
    }

    fn read_line_raw(&mut self, buf: &mut String) -> Result<usize> {
        if !self.is_chunked {
            return Ok(self.reader.read_line(buf)?);
        }
        if self.chunk_done {
            return Ok(0);
        }
        if self.chunk_remaining == 0 {
            let mut size_line = String::new();
            self.reader.read_line(&mut size_line)?;
            let size_str = size_line.trim().split(';').next().unwrap_or("0");
            let size = usize::from_str_radix(size_str, 16).map_err(|e| error(e))?;
            if size == 0 {
                self.chunk_done = true;
                return Ok(0);
            }
            self.chunk_remaining = size;
        }
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(0);
        }
        let consumed = line.len().min(self.chunk_remaining);
        self.chunk_remaining -= consumed;
        if self.chunk_remaining == 0 {
            let mut crlf = [0u8; 2];
            let _ = self.reader.read_exact(&mut crlf);
        }
        buf.push_str(&line);
        Ok(n)
    }
}

impl<R: Read> Iterator for OwnedEventIter<R> {
    type Item = Result<SseEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut event_type: Option<String> = None;
        let mut data_lines: Vec<String> = Vec::new();
        let mut id: Option<String> = None;
        let mut retry: Option<u64> = None;
        let mut got_any = false;

        loop {
            let mut line = String::new();
            match self.read_line_raw(&mut line) {
                Ok(0) => {
                    if got_any && !data_lines.is_empty() {
                        break;
                    }
                    return None;
                }
                Err(e) => return Some(Err(e)),
                Ok(_) => {}
            }

            let line = line.trim_end_matches('\n').trim_end_matches('\r');

            if line.is_empty() {
                if got_any && !data_lines.is_empty() {
                    break;
                }
                continue;
            }

            if line.starts_with(':') {
                continue;
            }

            got_any = true;

            let (field, value) = if let Some(pos) = line.find(':') {
                let f = &line[..pos];
                let v = line[pos + 1..]
                    .strip_prefix(' ')
                    .unwrap_or(&line[pos + 1..]);
                (f, v)
            } else {
                (line, "")
            };

            match field {
                "event" => event_type = Some(value.to_string()),
                "data" => data_lines.push(value.to_string()),
                "id" => id = Some(value.to_string()),
                "retry" => retry = value.parse().ok(),
                _ => {}
            }
        }

        Some(Ok(SseEvent {
            event: event_type,
            data: data_lines.join("\n"),
            id,
            retry,
        }))
    }
}
