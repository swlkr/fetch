use crate::{Headers, Response, Result, headers, parse_response, parse_url};
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;

pub struct Request<'a> {
    method: &'static str,
    url: &'a str,
    headers: Headers,
    body: &'a [u8],
}

impl<'a> Request<'a> {
    pub fn new(method: &'static str, url: &'a str) -> Self {
        Request {
            method,
            url,
            headers: headers(),
            body: b"",
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers = self.headers.push(name, value);
        self
    }

    pub fn body(mut self, bytes: &'a [u8]) -> Self {
        self.body = bytes;
        self
    }

    pub fn json(mut self, bytes: &'a [u8]) -> Self {
        self.body = bytes;
        self.headers = self.headers.push("content-type", "application/json");
        self
    }

    pub fn sse(mut self) -> Self {
        self.headers = self.headers.push("accept", "text/event-stream");
        self
    }

    pub fn response(self) -> Result<Response<TcpStream>> {
        let url = parse_url(self.url);
        let addr = format!("{}:{}", url.host, url.port);
        let stream = TcpStream::connect(&addr)?;

        let mut request: Vec<u8> = vec![];
        request.extend(self.method.as_bytes());
        request.extend(b" /");
        request.extend(url.path.as_bytes());
        request.extend(b" HTTP/1.1\r\n");
        request.extend(b"Host: ");
        request.extend(url.host.as_bytes());
        request.extend(b"\r\n");

        if self.method == "POST" {
            request.extend(b"Content-Length: ");
            request.extend(self.body.len().to_string().as_bytes());
            request.extend(b"\r\n");
        }

        for header in &self.headers.0 {
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

        if self.headers.get("connection").is_none() {
            request.extend(b"Connection: close\r\n");
        }

        request.extend(b"\r\n");

        send(stream, &request, self.body)
    }
}

fn send(mut stream: TcpStream, request: &[u8], body: &[u8]) -> Result<Response<TcpStream>> {
    stream.write_all(request)?;
    stream.write_all(body)?;
    stream.flush()?;

    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1];
    while let Ok(size) = stream.read(&mut tmp) {
        if size == 0 {
            return Err(crate::error("Unexpected eof"));
        }
        buf.push(tmp[0]);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
    }

    let head = parse_response(&buf)?;
    let reader = BufReader::new(stream);
    let response = Response::from_parts(head, reader);
    Ok(response)
}
