mod error;

pub use error::{Error, Result, error};
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub struct ResponseHead {
    pub status: u16,
    pub reason: String,
    pub headers: Headers,
    pub content_length: Option<usize>,
    pub is_chunked: bool,

    pub body_offset: usize,
}

pub struct Response<R: Read> {
    pub status: u16,
    pub reason: String,
    pub headers: Headers,
    reader: BufReader<R>,
    content_length: Option<usize>,
    is_chunked: bool,
}

pub struct Headers(Vec<Header>);

impl Headers {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn push(mut self, name: impl fmt::Display, value: impl fmt::Display) -> Self {
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

pub struct Header {
    name: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

pub struct ChunkIter<'a, R: Read> {
    reader: &'a mut BufReader<R>,
    done: bool,
}

impl<'a, R: Read> Iterator for ChunkIter<'a, R> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut size_line = String::new();
        if let Err(e) = self.reader.read_line(&mut size_line) {
            return Some(Err(error(e)));
        }
        let size_str = size_line.trim();
        let size_str = size_str.split(';').next().unwrap_or("0");
        let size = match usize::from_str_radix(size_str, 16) {
            Ok(s) => s,
            Err(e) => return Some(Err(error(e))),
        };
        if size == 0 {
            self.done = true;
            let mut trailing = String::new();
            let _ = self.reader.read_line(&mut trailing);
            return None;
        }
        let mut buf = vec![0u8; size];
        if let Err(e) = self.reader.read_exact(&mut buf) {
            return Some(Err(error(e)));
        }
        let mut crlf = [0u8; 2];
        if let Err(e) = self.reader.read_exact(&mut crlf) {
            return Some(Err(error(e)));
        }
        Some(Ok(buf))
    }
}

pub struct EventIter<'a, R: Read> {
    reader: &'a mut BufReader<R>,
    is_chunked: bool,
    chunk_remaining: usize,
    chunk_done: bool,
}

impl<'a, R: Read> EventIter<'a, R> {
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
            let size = usize::from_str_radix(size_str, 16)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
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

impl<'a, R: Read> Iterator for EventIter<'a, R> {
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
            for chunk in self.chunk() {
                result.extend(chunk?);
            }
            Ok(result)
        } else {
            let mut buf = Vec::new();
            self.reader.read_to_end(&mut buf)?;
            Ok(buf)
        }
    }

    pub fn chunk(&mut self) -> ChunkIter<'_, R> {
        ChunkIter {
            reader: &mut self.reader,
            done: false,
        }
    }

    pub fn event(&mut self) -> EventIter<'_, R> {
        EventIter {
            reader: &mut self.reader,
            is_chunked: self.is_chunked,
            chunk_remaining: 0,
            chunk_done: false,
        }
    }
}

pub fn parse_response(data: &[u8]) -> Result<ResponseHead> {
    let header_end = find_header_end(data)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no header terminator found"))?;

    let head = &data[..header_end];
    let head_str =
        std::str::from_utf8(head).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut lines = head_str.split("\r\n");

    let status_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty response"))?;
    let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err(error("malformed status line"));
    }
    let status: u16 = parts[1]
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let reason = parts.get(2).unwrap_or(&"").trim().to_string();

    let mut headers = vec![];
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let name = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim().to_string();
            headers.push(Header { name, value });
        }
    }

    let content_length = headers
        .iter()
        .find(|h| h.name == "content-length")
        .and_then(|h| h.value.parse::<usize>().ok());

    let is_chunked = headers
        .iter()
        .find(|h| h.name == "transfer-encoding")
        .map(|h| h.value.to_lowercase().contains("chunked"))
        .unwrap_or(false);

    let body_offset = header_end + 4;

    Ok(ResponseHead {
        status,
        reason,
        headers: Headers(headers),
        content_length,
        is_chunked,
        body_offset,
    })
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

struct Url<'a> {
    host: &'a str,
    port: &'a str,
    path: &'a str,
}

fn parse_url<'a>(url: &'a str) -> Url<'a> {
    let rest = match url.strip_prefix("http://") {
        Some(rest) => rest,
        None => url,
    };

    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, path),
        None => ("localhost", "/"),
    };

    let (host, port) = match host.split_once(':') {
        Some((host, port)) => (host, port),
        None => (host, "80"),
    };

    Url { host, port, path }
}

fn send_and_handle<F, T>(
    mut stream: TcpStream,
    request: &[u8],
    body: &[u8],
    callback: F,
) -> Result<T>
where
    F: FnOnce(Response<TcpStream>) -> Result<T>,
{
    stream.write_all(request)?;
    stream.write_all(body)?;
    stream.flush()?;

    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1];
    while let Ok(size) = stream.read(&mut tmp) {
        if size == 0 {
            return Err(error("Unexpected eof"));
        }
        buf.push(tmp[0]);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
    }

    let head = parse_response(&buf)?;
    let reader = BufReader::new(stream);
    let response = Response::from_parts(head, reader);
    callback(response)
}

pub fn post<F, T>(url: &str, headers: &Headers, body: &[u8], callback: F) -> Result<T>
where
    F: FnOnce(Response<TcpStream>) -> Result<T>,
{
    let Url {
        host, path, port, ..
    } = parse_url(url);
    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect(&addr)?;
    let mut request: Vec<u8> = vec![];
    request.extend(b"POST /");
    request.extend(path.as_bytes());
    request.extend(b"HTTP/1.1\r\n");
    request.extend(b"Host: ");
    request.extend(host.as_bytes());
    request.extend(b"\r\nContent-Length: ");
    request.extend(body.len().to_string().as_bytes());
    request.extend(b"\r\n");

    for header in &headers.0 {
        if header.name.eq_ignore_ascii_case("host")
            || header.name.eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        request.extend(header.name.as_bytes());
        request.extend(b": ");
        request.extend(header.value.as_bytes());
        request.extend(b"\r\n");
    }
    if !headers.get("connection").is_none() {
        request.extend(b"Connection: close\r\n");
    }
    request.extend(b"\r\n");

    send_and_handle(stream, &request, body, callback)
}

pub fn get<F, T>(url: &str, headers: &Headers, callback: F) -> Result<T>
where
    F: FnOnce(Response<TcpStream>) -> Result<T>,
{
    let Url { host, port, path } = parse_url(url);
    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect(&addr)?;

    let mut request = format!("GET /{} HTTP/1.1\r\n", path);
    request.push_str(&format!("Host: {}\r\n", host));

    for header in &headers.0 {
        if header.name.to_lowercase() == "host" {
            continue;
        }
        request.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    if headers.get("connection").is_none() {
        request.push_str("Connection: close\r\n");
    }
    request.push_str("\r\n");

    send_and_handle(stream, request.as_bytes(), &[], callback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn mock_response(raw: &[u8]) -> Response<Cursor<Vec<u8>>> {
        let head = parse_response(raw).expect("failed to parse response head");
        let body_bytes = raw[head.body_offset..].to_vec();
        let reader = BufReader::new(Cursor::new(body_bytes));
        Response::from_parts(head, reader)
    }

    // ── parse_response (pure &[u8] → ResponseHead) ───────────────────

    #[test]
    fn test_parse_head_status_and_reason() {
        let raw = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        let head = parse_response(raw).unwrap();
        assert_eq!(head.status, 404);
        assert_eq!(head.reason, "Not Found");
        assert_eq!(head.content_length, Some(0));
        assert!(!head.is_chunked);
    }

    #[test]
    fn test_parse_head_headers() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Custom: test\r\n\r\n";
        let head = parse_response(raw).unwrap();
        assert_eq!(
            head.headers.get("content-type").unwrap(),
            "application/json"
        );
        assert_eq!(head.headers.get("x-custom").unwrap(), "test");
    }

    #[test]
    fn test_parse_head_chunked_flag() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        let head = parse_response(raw).unwrap();
        assert!(head.is_chunked);
        assert_eq!(head.content_length, None);
    }

    #[test]
    fn test_parse_head_body_offset() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let head = parse_response(raw).unwrap();
        assert_eq!(&raw[head.body_offset..], b"hello");
    }

    #[test]
    fn test_parse_head_missing_terminator() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n";
        assert!(parse_response(raw).is_err());
    }

    // ── body ──────────────────────────────────────────────────────────

    #[test]
    fn test_body_content_length() {
        let res = mock_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
        assert_eq!(res.status, 200);
        assert_eq!(res.body().unwrap(), b"hello");
    }

    #[test]
    fn test_body_empty() {
        let res = mock_response(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(res.status, 204);
        assert!(res.body().unwrap().is_empty());
    }

    #[test]
    fn test_body_headers_roundtrip() {
        let res = mock_response(
            b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nX-Req-Id: abc\r\n\r\n{}",
        );
        assert_eq!(res.status, 201);
        assert_eq!(res.reason, "Created");
        assert_eq!(res.headers.get("content-type").unwrap(), "application/json");
        assert_eq!(res.headers.get("x-req-id").unwrap(), "abc");
        assert_eq!(res.body().unwrap(), b"{}");
    }

    // ── chunked ───────────────────────────────────────────────────────

    #[test]
    fn test_chunked_iter() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let mut res = mock_response(raw);
        let chunks: Vec<Vec<u8>> = res.chunk().collect::<Result<_>>().unwrap();
        assert_eq!(chunks, vec![b"hello".to_vec(), b" world".to_vec()]);
    }

    #[test]
    fn test_chunked_body_fallback() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     3\r\nabc\r\n4\r\ndefg\r\n0\r\n\r\n";
        let res = mock_response(raw);
        assert_eq!(res.body().unwrap(), b"abcdefg");
    }

    #[test]
    fn test_chunk_extensions_ignored() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     5;ext=val\r\nhello\r\n0\r\n\r\n";
        let mut res = mock_response(raw);
        let chunks: Vec<Vec<u8>> = res.chunk().collect::<Result<_>>().unwrap();
        assert_eq!(chunks, vec![b"hello".to_vec()]);
    }

    // ── SSE ───────────────────────────────────────────────────────────

    #[test]
    fn test_sse_basic() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                     event: message\ndata: hello\n\nevent: update\ndata: line1\ndata: line2\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.event().collect::<Result<_>>().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].data, "hello");
        assert_eq!(events[1].event.as_deref(), Some("update"));
        assert_eq!(events[1].data, "line1\nline2");
    }

    #[test]
    fn test_sse_id_and_retry() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\nid: 42\nretry: 3000\ndata: ping\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.event().collect::<Result<_>>().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert_eq!(events[0].retry, Some(3000));
        assert_eq!(events[0].data, "ping");
    }

    #[test]
    fn test_sse_comments_ignored() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\n: comment\ndata: actual\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.event().collect::<Result<_>>().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "actual");
    }

    #[test]
    fn test_sse_data_only() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\ndata: just data\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.event().collect::<Result<_>>().unwrap();
        assert_eq!(events[0].event, None);
        assert_eq!(events[0].data, "just data");
    }

    // ── URL parsing ───────────────────────────────────────────────────

    #[test]
    fn test_parse_url_http() {
        let url = parse_url("http://example.com:8080/api/v1");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, "8080");
        assert_eq!(url.path, "api/v1");
    }

    #[test]
    fn test_parse_url_default_port() {
        let url = parse_url("http://example.com/test");
        assert_eq!(url.port, "80");
    }
}
