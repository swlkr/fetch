# fetch

I never quit trying to make fetch happen

## Quickstart

```rust
use fetch::{post, get};

let headers = headers().push("content-type", "application/json");

// Content-Length
let body = post("http://localhost1234", &headers, b"{\"key\":\"val\"}", |res| {
    assert_eq!(res.status, 200);
    res.body()
}).unwrap();

// Transfer-Encoding: Chunked
post("http://localhost:1234", &headers, b"", |mut res| {
    for chunk in res.chunk() {
        let data = chunk?;
        print!("{}", String::from_utf8_lossy(&data));
    }
    Ok(())
}).unwrap();

// Sserver sent events
let headers = headers()
                .push("content-type", "application/json");
                .push("accept", "text/event-stream");

post("http://localhost:1234", &headers, b"{\"prompt\":\"hi\"}", |mut res| {
    for event in res.event() {
        let ev = event?;
        if let Some(ref name) = ev.event {
            print!("[{}] ", name);
        }
        println!("{}", ev.data);
    }
    Ok(())
}).unwrap();

let body = get("http://localhost:1234", headers(), |res| {
    res.body()
}).unwrap();
```
