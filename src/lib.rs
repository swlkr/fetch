mod chunk_iter;
mod error;
mod event_iter;
mod header;
mod headers;
mod request;
mod response;
mod response_head;
mod sse_event;
mod url;

pub use error::{Error, Result, error};
pub use header::Header;
pub use headers::{Headers, headers};
pub use request::Request;
pub use response::Response;
pub use response_head::ResponseHead;
pub use sse_event::SseEvent;
use std::io;
pub use url::Url;

pub fn post(url: &str) -> Request<'_> {
    Request::new("POST", url)
}

pub fn get(url: &str) -> Request<'_> {
    Request::new("GET", url)
}

pub(crate) fn parse_response(data: &[u8]) -> Result<ResponseHead> {
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

pub(crate) fn parse_url(url: &str) -> Url<'_> {
    let rest = match url.strip_prefix("http://") {
        Some(rest) => rest,
        None => url,
    };

    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, path),
        None => (rest, "/"),
    };

    let (host, port) = match host.split_once(':') {
        Some((host, port)) => (host, port),
        None => (host, "80"),
    };

    Url { host, port, path }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufReader, Cursor, Read, Write},
        net::TcpListener,
    };

    fn with_response(bytes: &'static [u8]) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr().unwrap();
        let port = addr.port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0; 1024];
                if let Ok(_size) = stream.read(&mut buffer) {
                    stream.write_all(bytes).unwrap();
                }
            }
        });
        Ok(format!("http://localhost:{port}"))
    }

    fn mock_response(raw: &[u8]) -> Response<Cursor<Vec<u8>>> {
        let head = parse_response(raw).expect("failed to parse response head");
        let body_bytes = raw[head.body_offset..].to_vec();
        let reader = BufReader::new(Cursor::new(body_bytes));
        Response::from_parts(head, reader)
    }

    #[test]
    fn test_get() -> Result<()> {
        let addr = with_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Custom: test\r\n\r\n",
        )?;
        let response = get(&addr).response()?;
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type"),
            Some("application/json")
        );
        assert_eq!(response.headers.get("x-custom"), Some("test"));
        Ok(())
    }

    #[test]
    fn test_get_content_length() -> Result<()> {
        let addr = with_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")?;
        let res = get(&addr).response()?;
        assert_eq!(res.status, 200);
        assert_eq!(res.headers.get("content-length"), Some("5"));
        assert_eq!(res.body().unwrap(), b"hello");
        Ok(())
    }

    #[test]
    fn test_get_sse() -> Result<()> {
        let addr = with_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                     event: message\ndata: hello\n\nevent: update\ndata: line1\ndata: line2\n\n",
        )?;
        let mut res = get(&addr).response()?;
        assert_eq!(res.status, 200);
        assert_eq!(res.headers.get("content-type"), Some("text/event-stream"));
        let events: Vec<SseEvent> = res.events().collect::<Result<_>>().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].data, "hello");
        assert_eq!(events[1].event.as_deref(), Some("update"));
        assert_eq!(events[1].data, "line1\nline2");
        Ok(())
    }

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

    #[test]
    fn test_chunked_iter() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let mut res = mock_response(raw);
        let chunks: Vec<Vec<u8>> = res.chunks().collect::<Result<_>>().unwrap();
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
        let chunks: Vec<Vec<u8>> = res.chunks().collect::<Result<_>>().unwrap();
        assert_eq!(chunks, vec![b"hello".to_vec()]);
    }

    #[test]
    fn test_sse_basic() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                     event: message\ndata: hello\n\nevent: update\ndata: line1\ndata: line2\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.events().collect::<Result<_>>().unwrap();
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
        let events: Vec<SseEvent> = res.events().collect::<Result<_>>().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert_eq!(events[0].retry, Some(3000));
        assert_eq!(events[0].data, "ping");
    }

    #[test]
    fn test_sse_comments_ignored() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\n: comment\ndata: actual\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.events().collect::<Result<_>>().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "actual");
    }

    #[test]
    fn test_sse_data_only() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\ndata: just data\n\n";
        let mut res = mock_response(raw);
        let events: Vec<SseEvent> = res.events().collect::<Result<_>>().unwrap();
        assert_eq!(events[0].event, None);
        assert_eq!(events[0].data, "just data");
    }

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
