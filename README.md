# fetch

I never quit trying to make fetch happen

## Quickstart

```rust
use fetch::{post, get};

// POST with JSON convenience
let res = post("http://localhost:1234")
    .header("x-api-key", "key")
    .json(b"{\"key\":\"val\"}")
    .response()?;
assert_eq!(res.status, 200);
let body = res.body()?;

// POST with raw body
let res = post("http://localhost:1234")
    .header("content-type", "application/json")
    .body(b"{\"key\":\"val\"}")
    .response()?;

// GET
let res = get("http://localhost:1234")
    .header("accept", "text/html")
    .response()?;
let body = res.body()?;

// Transfer-Encoding: Chunked
let mut res = post("http://localhost:1234")
    .json(b"{}")
    .response()?;
for chunk in res.chunks() {
    let data = chunk?;
    print!("{}", String::from_utf8_lossy(&data));
}

// Server sent events
let mut res = post("http://localhost:1234")
    .json(b"{\"prompt\":\"hi\"}")
    .sse()
    .response()?;
for event in res.events() {
    let ev = event?;
    if let Some(ref name) = ev.event {
        print!("[{}] ", name);
    }
    println!("{}", ev.data);
}
```
