//! Browser bridge: the OpenFlow Chrome extension (browser-extension/) reports who is talking in a Google Meet tab
//! and Meet's own caption lines, so far-side lines get names. Plain HTTP on 127.0.0.1, one endpoint, small bounded
//! JSON bodies; only requests whose Origin is our extension (fixed ID from the manifest key) are accepted, so a web
//! page cannot feed it. Losing the extension only loses names; capture never depends on it.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const PORT: u16 = 8770;
/// chrome-extension://<ID>, derived from the public key in browser-extension/manifest.json.
pub const EXTENSION_ID: &str = "mbddbmiendbipklcehlgelanomjpoppf";
const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Event {
    /// The active remote speaker (None: nobody), the user's own name and everyone in the call.
    Speaker {
        active: Option<String>,
        #[serde(rename = "self")]
        self_name: Option<String>,
        #[serde(default)]
        participants: Vec<String>,
    },
    /// One caption block as Meet shows it: `name` is saying `text` (grows while they talk).
    Caption { name: String, text: String },
    /// The tab joined or left a call.
    Call {
        #[serde(rename = "inCall")]
        in_call: bool,
    },
    /// A Meet tab (or the extension's popup) checking in, so the app can show the extension as connected.
    Hello,
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// Serves until the app exits. A port already in use (a second copy of the app) is logged and ignored.
pub fn spawn(sink: Sink) {
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(target: "bridge", "browser bridge unavailable on port {PORT}: {e}");
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let sink = Arc::clone(&sink);
            tauri::async_runtime::spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(5), serve(stream, sink)).await;
            });
        }
    });
}

async fn serve(mut stream: TcpStream, sink: Sink) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(2048);
    let header_end = loop {
        let mut chunk = [0u8; 2048];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEADER {
            return respond(&mut stream, 431, "").await;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let request = parse_head(&head);
    let origin_ok = request.origin.as_deref() == Some(&format!("chrome-extension://{EXTENSION_ID}"));
    if request.method == "OPTIONS" {
        return respond(&mut stream, if origin_ok { 204 } else { 403 }, "").await;
    }
    if request.method != "POST" || request.path != "/meet" {
        return respond(&mut stream, 404, "").await;
    }
    if !origin_ok {
        return respond(&mut stream, 403, "").await;
    }
    if request.length > MAX_BODY {
        return respond(&mut stream, 413, "").await;
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < request.length {
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(request.length);
    match parse_event(&body) {
        Some(e) => {
            sink(e);
            respond(&mut stream, 204, "").await
        }
        None => respond(&mut stream, 400, "").await,
    }
}

struct Head {
    method: String,
    path: String,
    origin: Option<String>,
    length: usize,
}

fn parse_head(head: &str) -> Head {
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    let (mut origin, mut length) = (None, 0);
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "origin" => origin = Some(v.trim().to_string()),
                "content-length" => length = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    Head { method, path, origin, length }
}

/// Bounded: names are cut to 60 characters, captions to 2000, and at most 100 participants.
pub fn parse_event(body: &[u8]) -> Option<Event> {
    let e: Event = serde_json::from_slice(body).ok()?;
    let cut = |s: String, n: usize| s.chars().take(n).collect::<String>();
    Some(match e {
        Event::Speaker { active, self_name, participants } => Event::Speaker {
            active: active.map(|s| cut(s, 60)),
            self_name: self_name.map(|s| cut(s, 60)),
            participants: participants.into_iter().take(100).map(|s| cut(s, 60)).collect(),
        },
        Event::Caption { name, text } => Event::Caption { name: cut(name, 60), text: cut(text, 2000) },
        other => other,
    })
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Request Header Fields Too Large",
    };
    let cors = format!(
        "Access-Control-Allow-Origin: chrome-extension://{EXTENSION_ID}\r\nAccess-Control-Allow-Methods: POST\r\nAccess-Control-Allow-Headers: Content-Type\r\n"
    );
    let msg = format!("HTTP/1.1 {status} {reason}\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(msg.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_parse_and_are_bounded() {
        let e = parse_event(br#"{"type":"speaker","active":"Priya","self":"Me Person","participants":["Priya","Rahul"]}"#);
        assert_eq!(
            e,
            Some(Event::Speaker {
                active: Some("Priya".into()),
                self_name: Some("Me Person".into()),
                participants: vec!["Priya".into(), "Rahul".into()]
            })
        );
        let long = "x".repeat(5000);
        let body = format!(r#"{{"type":"caption","name":"Priya","text":"{long}"}}"#);
        match parse_event(body.as_bytes()) {
            Some(Event::Caption { text, .. }) => assert_eq!(text.len(), 2000),
            other => panic!("{other:?}"),
        }
        assert_eq!(parse_event(br#"{"type":"call","inCall":true}"#), Some(Event::Call { in_call: true }));
        assert_eq!(parse_event(br#"{"type":"hello"}"#), Some(Event::Hello));
        assert_eq!(parse_event(br#"{"type":"speaker"}"#), Some(Event::Speaker { active: None, self_name: None, participants: vec![] }));
        assert_eq!(parse_event(b"nonsense"), None);
        assert_eq!(parse_event(br#"{"type":"evil"}"#), None);
    }

    #[test]
    fn head() {
        let h = parse_head("POST /meet HTTP/1.1\r\nHost: x\r\norigin: chrome-extension://abc\r\nContent-Length: 12\r\n\r\n");
        assert_eq!((h.method.as_str(), h.path.as_str(), h.origin.as_deref(), h.length), ("POST", "/meet", Some("chrome-extension://abc"), 12));
    }

    #[tokio::test]
    async fn only_our_extension_is_heard() {
        let got: Arc<std::sync::Mutex<Vec<Event>>> = Arc::default();
        let g = Arc::clone(&got);
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (s, _) = listener.accept().await.unwrap();
                let g = Arc::clone(&g);
                serve(s, Arc::new(move |e| g.lock().unwrap().push(e))).await.unwrap();
            }
        });
        let body = r#"{"type":"caption","name":"Priya","text":"hello"}"#;
        let send = |origin: String| {
            let body = body.to_string();
            async move {
                let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                let req = format!("POST /meet HTTP/1.1\r\nOrigin: {origin}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                s.write_all(req.as_bytes()).await.unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).await.unwrap();
                out
            }
        };
        assert!(send("https://evil.example".into()).await.starts_with("HTTP/1.1 403"));
        assert!(got.lock().unwrap().is_empty());
        assert!(send(format!("chrome-extension://{EXTENSION_ID}")).await.starts_with("HTTP/1.1 204"));
        assert_eq!(got.lock().unwrap().len(), 1);
    }
}
