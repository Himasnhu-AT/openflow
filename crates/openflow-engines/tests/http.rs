//! HTTP behaviour against a local mock server (no network).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use openflow_core::traits::{Examples, PolishBackend};
use openflow_core::Config;
use openflow_engines::cloud::{Cloud, Scope};
use openflow_engines::downloader::{DownloadEvent, Downloader};
use openflow_engines::polisher::Polisher;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
struct Req {
    head: String,
    body: Vec<u8>,
}

struct Reply {
    status: u16,
    content_type: &'static str,
    /// Written one after another with a flush between (for SSE).
    parts: Vec<Vec<u8>>,
}

fn reply(status: u16, body: &str) -> Reply {
    Reply { status, content_type: "application/json", parts: vec![body.as_bytes().to_vec()] }
}

/// Serves `replies` in order, one per connection (the last one repeats). Returns the port and the requests seen.
async fn serve(replies: Vec<Reply>) -> (u16, Arc<Mutex<Vec<Req>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let replies = Arc::new(replies);
    tokio::spawn(async move {
        let mut i = 0usize;
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let r = &replies[i.min(replies.len() - 1)];
            i += 1;
            let mut buf = Vec::new();
            let mut tmp = [0u8; 65536];
            let head_end = loop {
                let n = sock.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break None;
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(p + 4);
                }
            };
            let Some(head_end) = head_end else { continue };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let len = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                .unwrap_or(0);
            while buf.len() < head_end + len {
                let n = sock.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            log.lock().unwrap().push(Req { head, body: buf[head_end..].to_vec() });
            let total: usize = r.parts.iter().map(Vec::len).sum();
            let h = format!(
                "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n",
                r.status, r.content_type
            );
            let _ = sock.write_all(h.as_bytes()).await;
            for p in &r.parts {
                let _ = sock.write_all(p).await;
                let _ = sock.flush().await;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let _ = sock.shutdown().await;
        }
    });
    (port, seen)
}

fn local_config(port: u16) -> Config {
    Config { llama_port: port, cloud_enabled: false, llm_backend: "llamacpp".into(), ..Config::default() }
}

#[tokio::test]
async fn health_and_complete() {
    let (port, seen) = serve(vec![
        reply(200, "{}"),
        reply(
            200,
            r#"{"choices":[{"message":{"content":"<think>\n</think>\n\n Hello there. "}}],"timings":{"prompt_n":5,"prompt_ms":10.2,"predicted_n":3,"predicted_ms":7.9,"draft_n":4,"draft_n_accepted":3}}"#,
        ),
    ])
    .await;
    let p = Polisher::new(local_config(port), Scope::Dictation);
    assert!(p.is_healthy().await);
    let out = p.complete("sys", "usr", 1024, Duration::from_secs(5)).await.unwrap();
    assert_eq!(out, "Hello there.");
    assert_eq!(p.last_timings(), "prompt 5 tok in 10 ms, generated 3 tok in 7 ms, draft 3/4");
    let reqs = seen.lock().unwrap();
    assert!(reqs[0].head.starts_with("GET /health "));
    assert!(reqs[1].head.starts_with("POST /v1/chat/completions "));
    let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["content"], "sys");
}

#[tokio::test]
async fn unhealthy_when_nothing_listens() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let p = Polisher::new(local_config(port), Scope::Dictation);
    assert!(!p.is_healthy().await);
    assert!(!p.wait_until_ready(Duration::from_millis(600)).await);
}

#[tokio::test]
async fn polish_streams_partials_and_falls_back_to_raw() {
    let sse = [
        "data: {\"choices\":[{\"delta\":{\"content\":\"<think>\\n\\n\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"</think>\\n\\nHi\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" Alex.\"}}]}\n\ndata: {\"choices\":[{\"delta\":{}}],\"timings\":{\"prompt_n\":1,\"prompt_ms\":2.0,\"predicted_n\":3,\"predicted_ms\":4.0}}\n\n",
        "data: [DONE]\n\n",
    ];
    let (port, seen) = serve(vec![
        Reply { status: 200, content_type: "text/event-stream", parts: sse.iter().map(|s| s.as_bytes().to_vec()).collect() },
        reply(200, r#"{"choices":[{"message":{"content":"<think></think>  "}}]}"#),
    ])
    .await;
    let p = Polisher::new(local_config(port), Scope::Dictation);
    let partials = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = partials.clone();
    let f = move |s: &str| sink.lock().unwrap().push(s.to_string());
    let tones = [("casual".to_string(), "c".to_string())].into_iter().collect();
    let out = p.polish("um hi alex", "casual", &tones, &[], "", &Examples::new(), None, Some(&f)).await.unwrap();
    assert_eq!(out, "Hi Alex.");
    assert_eq!(*partials.lock().unwrap(), vec!["Hi".to_string(), "Hi Alex.".to_string()]);
    assert!(p.last_timings().starts_with("prompt 1 tok in 2 ms, generated 3 tok in 4 ms, first chunk after"));
    // No partial callback -> non-streaming; an empty reply falls back to the raw transcript.
    let out = p.polish("um hi alex", "casual", &tones, &[], "", &Examples::new(), None, None).await.unwrap();
    assert_eq!(out, "um hi alex");
    let reqs = seen.lock().unwrap();
    assert!(reqs[0].head.to_ascii_lowercase().contains("accept-encoding: identity"));
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][1]["content"], "[tone: casual]\num hi alex");
}

fn cloud_config() -> Config {
    Config {
        llm_backend: "llamacpp".into(),
        cloud_enabled: true,
        cloud_api_key: " gsk_test ".into(),
        cloud_dictation: "all".into(),
        cloud_llm_model: "qwen/qwen3-32b".into(),
        ..Config::default()
    }
}

#[tokio::test]
async fn cloud_complete_falls_back_on_unavailable_models() {
    let (port, seen) = serve(vec![
        reply(404, r#"{"error":{"message":"The model `qwen/qwen3-32b` does not exist","code":"model_not_found"}}"#),
        reply(429, r#"{"error":{"message":"Rate limit reached"}}"#),
        reply(200, r#"{"choices":[{"message":{"content":"Done."}}]}"#),
    ])
    .await;
    let cloud = Cloud::with_env(&cloud_config(), Scope::Dictation, false, None)
        .unwrap()
        .with_base_url(format!("http://127.0.0.1:{port}"));
    assert_eq!(cloud.complete("s", "u", 1024).await.unwrap(), "Done.");
    let reqs = seen.lock().unwrap();
    let models: Vec<String> = reqs
        .iter()
        .map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).unwrap()["model"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(models, ["qwen/qwen3-32b", "openai/gpt-oss-120b", "openai/gpt-oss-20b"]);
    let h = reqs[0].head.to_ascii_lowercase();
    assert!(h.starts_with("post /chat/completions "));
    assert!(h.contains("authorization: bearer gsk_test\r\n"));
    assert!(h.contains("user-agent: openflow/0.1"));
}

#[tokio::test]
async fn cloud_complete_stops_on_other_errors() {
    let (port, seen) = serve(vec![reply(401, r#"{"error":{"message":"Invalid API Key"}}"#)]).await;
    let cloud = Cloud::with_env(&cloud_config(), Scope::Dictation, false, None)
        .unwrap()
        .with_base_url(format!("http://127.0.0.1:{port}"));
    let e = cloud.complete("s", "u", 10).await.unwrap_err().to_string();
    assert!(e.starts_with("Cloud model failed: "), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cloud_transcribe_multipart() {
    let (port, seen) = serve(vec![reply(200, r#"{"text":" Hello world.","segments":[{"text":" Hello world."}]}"#)]).await;
    let cloud = Cloud::with_env(&cloud_config(), Scope::Dictation, false, None)
        .unwrap()
        .with_base_url(format!("http://127.0.0.1:{port}"));
    let t = cloud.transcribe(&vec![0.1; 16000], None).await.unwrap();
    assert_eq!(t.trim(), "Hello world.");
    let r = &seen.lock().unwrap()[0];
    assert!(r.head.contains("multipart/form-data; boundary=openflow-"));
    let body = String::from_utf8_lossy(&r.body);
    assert!(body.contains("name=\"model\"\r\n\r\nwhisper-large-v3-turbo\r\n"));
    assert!(!body.contains("name=\"prompt\""));
    assert!(body.contains("filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF"));
}

#[tokio::test]
async fn downloader_streams_renames_and_ignores_duplicates() {
    let data = vec![7u8; 300_000];
    let (port, _) = serve(vec![Reply {
        status: 200,
        content_type: "application/octet-stream",
        parts: data.chunks(100_000).map(<[u8]>::to_vec).collect(),
    }])
    .await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.gguf"), b"old").unwrap();
    let events = Arc::new(Mutex::new(Vec::<DownloadEvent>::new()));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = events.clone();
    let d = Downloader::new(Arc::new(move |e: DownloadEvent| {
        if e.done {
            let _ = tx.send(());
        }
        sink.lock().unwrap().push(e);
    }));
    let url = format!("http://127.0.0.1:{port}/files/m.gguf");
    assert!(d.start("m.gguf", &url, dir.path()));
    assert!(!d.start("m.gguf", &url, dir.path()));
    tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap();
    assert_eq!(std::fs::read(dir.path().join("m.gguf")).unwrap(), data);
    assert!(!dir.path().join("m.gguf.download").exists());
    assert!(!d.is_downloading("m.gguf"));
    let ev = events.lock().unwrap();
    assert_eq!(ev[0].progress, Some(0.0));
    let last = ev.last().unwrap();
    assert!(last.done && last.error.is_none());
    assert_eq!(ev[ev.len() - 2].progress, Some(1.0));
}

#[tokio::test]
async fn downloader_reports_http_errors() {
    let (port, _) = serve(vec![reply(404, "nope")]).await;
    let dir = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let d = Downloader::new(Arc::new(move |e: DownloadEvent| {
        if e.done {
            let _ = tx.send(e);
        }
    }));
    d.start("x.bin", &format!("http://127.0.0.1:{port}/x.bin"), dir.path());
    let e = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap().unwrap();
    assert_eq!(e.error.as_deref(), Some("Download failed: HTTP 404"));
    assert!(!dir.path().join("x.bin").exists());
}

// ------------------------------------------------------------------------------------------------ Ollama

fn ollama_config(port: u16) -> Config {
    Config {
        cloud_enabled: false,
        llm_backend: "ollama".into(),
        ollama_url: format!("http://127.0.0.1:{port}"),
        ollama_model: "qwen3.5:4b".into(),
        ..Config::default()
    }
}

#[tokio::test]
async fn ollama_complete_sends_native_chat_without_thinking() {
    let body = r#"{"model":"qwen3.5:4b","message":{"role":"assistant","content":"see you thursday"},"done":true,"prompt_eval_count":900,"prompt_eval_duration":80000000,"eval_count":4,"eval_duration":60000000}"#;
    let (port, seen) = serve(vec![reply(200, body)]).await;
    let p = Polisher::new(ollama_config(port), Scope::Dictation);
    let out = p.complete("SYS", "USER", 1024, Duration::from_secs(5)).await.unwrap();
    assert_eq!(out, "see you thursday");
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.head.starts_with("POST /api/chat "));
    let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(v["model"], "qwen3.5:4b");
    assert_eq!(v["think"], false);
    assert_eq!(v["stream"], false);
    assert_eq!(v["keep_alive"], "30m");
    assert_eq!(v["messages"][0]["content"], "SYS");
    assert!(p.last_timings().contains("prompt 900 tok"));
}

#[tokio::test]
async fn ollama_polish_streams_ndjson_partials() {
    let lines = [
        r#"{"message":{"role":"assistant","content":"thursday "},"done":false}"#,
        r#"{"message":{"role":"assistant","content":"works"},"done":false}"#,
        r#"{"message":{"role":"assistant","content":""},"done":true,"eval_count":2}"#,
    ];
    let parts = lines.iter().map(|l| format!("{l}\n").into_bytes()).collect();
    let (port, _) = serve(vec![Reply { status: 200, content_type: "application/x-ndjson", parts }]).await;
    let p = Polisher::new(ollama_config(port), Scope::Dictation);
    let partials = Mutex::new(Vec::new());
    let cb = |s: &str| partials.lock().unwrap().push(s.to_string());
    let tones = Config::default().tones;
    let out = p
        .polish("um thursday works", "casual", &tones, &[], "", &Examples::new(), None, Some(&cb))
        .await
        .unwrap();
    assert_eq!(out, "thursday works");
    assert_eq!(partials.lock().unwrap().last().unwrap(), "thursday works");
}

#[tokio::test]
async fn ollama_start_requires_the_model_to_be_pulled() {
    let tags = r#"{"models":[{"name":"qwen3.5:9b","size":6600000000}]}"#;
    // /api/version, then /api/tags.
    let (port, _) = serve(vec![reply(200, r#"{"version":"0.34.1"}"#), reply(200, tags)]).await;
    let p = Polisher::new(ollama_config(port), Scope::Dictation);
    let err = p.start().await.unwrap_err().to_string();
    assert!(err.contains("qwen3.5:4b isn't downloaded"), "{err}");
}

#[tokio::test]
async fn ollama_model_errors_surface() {
    let (port, _) = serve(vec![reply(404, r#"{"error":"model 'qwen3.5:4b' not found"}"#)]).await;
    let p = Polisher::new(ollama_config(port), Scope::Dictation);
    let err = p.complete("S", "U", 10, Duration::from_secs(5)).await.unwrap_err().to_string();
    assert!(err.contains("not found"), "{err}");
}

#[tokio::test]
async fn ollama_refuses_cloud_models_as_the_local_engine() {
    let tags = r#"{"models":[{"name":"glm-5.2:cloud","remote_model":"glm-5.2","remote_host":"https://ollama.com","size":290}]}"#;
    let (port, _) = serve(vec![reply(200, r#"{"version":"0.34.1"}"#), reply(200, tags)]).await;
    let mut cfg = ollama_config(port);
    cfg.ollama_model = "glm-5.2:cloud".into();
    let err = Polisher::new(cfg, Scope::Dictation).start().await.unwrap_err().to_string();
    assert!(err.contains("cloud model"), "{err}");
}
