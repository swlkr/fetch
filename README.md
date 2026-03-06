# fetch

I never quit trying to make fetch happen

## Quickstart

```rust
use fetch::{post, get};

let headers = headers().push("content-type", "application/json");

// Content-Length
let res = post("http://localhost1234", &headers, b"{\"key\":\"val\"}")?;
assert_eq!(res.status, 200);
let body = res.body().unwrap();

// Transfer-Encoding: Chunked
let mut res = post("http://localhost:1234", &headers, b"")?;
for chunk in res.chunk() {
    let data = chunk?;
    print!("{}", String::from_utf8_lossy(&data));
}

// Server sent events
let headers = headers()
                .push("content-type", "application/json");
                .push("accept", "text/event-stream");

let res = post("http://localhost:1234", &headers, b"{\"prompt\":\"hi\"}")?;
for event in res.event() {
    let ev = event?;
    if let Some(ref name) = ev.event {
        print!("[{}] ", name);
    }
    println!("{}", ev.data);
}

// GET
let res = get("http://localhost:1234", &headers())?;
res.body().unwrap()
```
