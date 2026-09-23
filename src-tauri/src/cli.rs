//! Command-line tools (main.swift flags), run before the GUI starts. `dispatch` returns `Some(exit_code)` when a
//! tool ran. They use the same data directory, engines and rules as the app.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jez_core::model::Meeting;
use jez_core::store::parse_history;
use jez_core::traits::{AsrEngine, Examples, PolishBackend, SystemAudio};
use jez_core::{paths, pipeline, text_rules, wav, wer, Config, SAMPLE_RATE};
use jez_engines::cloud::{Cloud, Scope};
use jez_engines::live::LiveTranscriber;
use jez_engines::polisher::Polisher;
use jez_engines::prompts;
use jez_engines::sidecar::{Sidecar, SidecarAsr, SidecarSystemAudio};
use jez_platform::live_typer::LiveTyper;
use jez_platform::Platform;

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).cloned()
}

fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn value(args: &[String], prefix: &str) -> Option<String> {
    args.iter().find_map(|a| a.strip_prefix(prefix).map(String::from))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime")
}

fn exit(ok: bool) -> Option<i32> {
    Some(if ok { 0 } else { 1 })
}

pub fn dispatch(args: &[String]) -> Option<i32> {
    if has(args, "--selftest") {
        return exit(selftest());
    }
    if has(args, "--context") {
        return exit(context());
    }
    if let Some(path) = arg_after(args, "--livetest") {
        return exit(runtime().block_on(livetest(args, &path)));
    }
    if let Some(raw) = arg_after(args, "--polishtest") {
        return exit(runtime().block_on(polishtest(&raw)));
    }
    if let Some(tone) = arg_after(args, "--dumpprompt") {
        return exit(dumpprompt(&tone));
    }
    if has(args, "--enginetest") || has(args, "--mictest") {
        return exit(runtime().block_on(mictest(args)));
    }
    if has(args, "--notestest") {
        return exit(runtime().block_on(notestest()));
    }
    if has(args, "--compare") {
        let n = arg_after(args, "--compare").and_then(|n| n.parse().ok()).unwrap_or(20);
        return exit(runtime().block_on(compare(n)));
    }
    if let Some(name) = arg_after(args, "--tapprocs") {
        return exit(runtime().block_on(tapprocs(&name)));
    }
    if has(args, "--taptest") {
        return exit(runtime().block_on(taptest()));
    }
    if has(args, "--eval") {
        return exit(runtime().block_on(eval(has(args, "--no-boost"))));
    }
    None
}

fn load_config() -> Option<Config> {
    Config::load().map_err(|e| eprintln!("config: {e:#}")).ok()
}

fn history() -> Vec<jez_core::model::Dictation> {
    std::fs::read(paths::history_file()).map(|b| parse_history(&b)).unwrap_or_default()
}

/// Rule-layer checks, no models needed.
fn selftest() -> bool {
    let (mut passed, mut failures) = text_rules::selftest::self_test();
    let (p2, f2) = data_self_test();
    passed += p2;
    failures.extend(f2);
    for f in &failures {
        println!("FAIL {f}");
    }
    println!("selftest: {passed} passed, {} failed", failures.len());
    if let Some(ms) = Some(text_rules::selftest::rules_benchmark_ms()) {
        println!("rules: {ms:.2} ms per 60-word dictation");
    }
    failures.is_empty()
}

/// The SelfTest.swift checks that live outside the rules: segmenter and meeting to-dos.
fn data_self_test() -> (usize, Vec<String>) {
    let mut passed = 0;
    let mut failures = vec![];
    let mut check = |name: &str, got: String, want: String| {
        if got == want {
            passed += 1;
        } else {
            failures.push(format!("{name}: got {got:?}, want {want:?}"));
        }
    };
    let got: Arc<Mutex<Vec<(usize, f64)>>> = Arc::default();
    let g2 = Arc::clone(&got);
    let seg = jez_core::segmenter::Segmenter::new(Box::new(move |s, t| g2.lock().unwrap().push((s.len(), t))));
    let mut audio = vec![0f32; 16_000]; // 1 s silence
    audio.extend((0..24_000).map(|i| (i as f32 * 0.2).sin() * 0.3)); // 1.5 s tone
    audio.extend(vec![0f32; 16_000]); // 1 s silence
    for chunk in audio.chunks(150) {
        seg.push(chunk); // tap-sized chunks
    }
    let got = got.lock().unwrap().clone();
    check("segmenter utterances", got.len().to_string(), "1".into());
    check("segmenter start", got.first().map(|g| ((g.1 * 10.0) as i64).to_string()).unwrap_or("-1".into()), "7".into());

    let mut m = Meeting::new("t".into(), None, chrono::Utc::now());
    m.todos = vec![jez_core::model::MeetingTodo {
        id: uuid::Uuid::new_v4(),
        text: "Send the deck to Alex by Friday".into(),
        done: true,
    }];
    m.refresh_todos("## Summary\n- a\n## Decisions\n- None\n## Action items\n- Alex: review\n## Your to-dos\n- Send the deck to Alex by Friday\n- Book the venue\n");
    let todos: Vec<String> = m.todos.iter().map(|t| format!("{}|{}", t.text, t.done)).collect();
    check("todos parsed", format!("{todos:?}"), format!("{:?}", ["Send the deck to Alex by Friday|true", "Book the venue|false"]));
    m.refresh_todos("## Your to-dos\n- None");
    check("todos none", m.todos.len().to_string(), "0".into());
    (passed, failures)
}

/// What the field reader sees for the frontmost app (sizes only, no field text).
fn context() -> bool {
    let p = Platform::native();
    let c = p.field.capture();
    println!("trusted: {}", p.field.is_trusted(false));
    println!(
        "window title: {}\nbefore cursor: {} chars\nafter cursor: {} chars\nneeds leading space: {}",
        c.window_title,
        c.before.chars().count(),
        c.after.chars().count(),
        c.needs_leading_space()
    );
    true
}

fn dumpprompt(tone: &str) -> bool {
    let Some(config) = load_config() else { return false };
    print!(
        "{}",
        prompts::system_prompt(&config.tones, &config.dictionary, &config.memory, &Examples::new(), &config.learned_signals)
    );
    println!();
    println!("=====USER=====\n{}", prompts::user_message("TRANSCRIPT", tone, None));
    true
}

async fn asr(config: &Config, boost: bool) -> anyhow::Result<(Arc<Sidecar>, Arc<dyn AsrEngine>, LiveTranscriber)> {
    let sidecar = Sidecar::new();
    let engine: Arc<dyn AsrEngine> = Arc::new(SidecarAsr(Arc::clone(&sidecar)));
    engine.load(&config.asr_version).await?;
    let live = LiveTranscriber::new(Arc::clone(&engine));
    if boost {
        live.set_vocabulary(&config.dictionary, &[]).await?;
    }
    Ok((sidecar, engine, live))
}

/// Replays every corrected dictation with a clip through the current engines, prompts and rules.
async fn eval(no_boost: bool) -> bool {
    let Some(config) = load_config() else { return false };
    let store_clip = |d: &jez_core::model::Dictation| d.clip.as_ref().is_some_and(|c| paths::clips_dir().join(c).exists());
    let history = history();
    let cases: Vec<_> = history.iter().filter(|d| d.correction.is_some() && store_clip(d)).rev().cloned().collect();
    let no_clip = history.iter().filter(|d| d.correction.is_some() && !store_clip(d)).count();
    let no_correction = history.iter().filter(|d| d.correction.is_none() && store_clip(d)).count();
    println!(
        "eval: {} corrected dictations with audio ({no_clip} corrections without a clip, {no_correction} clips not corrected yet)",
        cases.len()
    );
    if cases.is_empty() {
        println!("Turn on Settings > Keep audio clips, dictate, then use the pencil on a History row to save what you meant.");
        return false;
    }
    let (_sidecar, engine, _live) = match asr(&config, !no_boost).await {
        Ok(a) => a,
        Err(e) => {
            println!("eval: engines failed: {e:#}");
            return false;
        }
    };
    let polisher = Polisher::new(config.clone(), Scope::Dictation);
    if !polisher.is_healthy().await {
        if let Err(e) = polisher.start().await {
            println!("eval: engines failed: {e:#}");
            return false;
        }
        if !polisher.wait_until_ready(Duration::from_secs(120)).await {
            println!("eval: llama-server not ready");
            return false;
        }
    }
    let pad = |s: &str, w: usize| format!("{s:<w$}");
    let pct = |x: f64| pad(&format!("{}%", (x * 100.0).round() as i64), 8);
    println!(
        "\n{}{}{}{}{}WER vs correction (lower is better; * = exact match)",
        pad("#", 4),
        pad("tone", 13),
        pad("heard", 8),
        pad("shipped", 8),
        pad("now", 8)
    );
    let (mut sum_heard, mut sum_shipped, mut sum_now, mut exact) = (0.0, 0.0, 0.0, 0);
    let mut misses = vec![];
    for (i, d) in cases.iter().enumerate() {
        let want = d.correction.clone().unwrap_or_default();
        let mut raw = d.raw.clone();
        let mut now = "(failed)".to_string();
        let result = async {
            let samples = wav::read(&paths::clips_dir().join(d.clip.as_ref().unwrap()))?;
            raw = engine.transcribe(&samples, !no_boost).await?;
            now = pipeline::run(&raw, &d.tone, &config, Some(&polisher as &dyn PolishBackend), &Examples::new(), None, None).await?;
            anyhow::Ok(())
        }
        .await;
        if let Err(e) = result {
            println!("  #{} failed: {e:#}", i + 1);
        }
        let heard = wer::word_error_rate(&want, &raw);
        let shipped = wer::word_error_rate(&want, &d.text);
        let score = wer::word_error_rate(&want, &now);
        sum_heard += heard;
        sum_shipped += shipped;
        sum_now += score;
        let is_exact = now == want;
        if is_exact {
            exact += 1;
        } else {
            misses.push((i + 1, want.clone(), raw.clone(), now.clone()));
        }
        println!(
            "{}{}{}{}{}{}",
            pad(&(i + 1).to_string(), 4),
            pad(&d.tone, 13),
            pct(heard),
            pct(shipped),
            pct(score),
            if is_exact { "*" } else { "" }
        );
    }
    let n = cases.len() as f64;
    println!(
        "{}{}{}{}exact {exact}/{}",
        pad("avg", 17),
        pct(sum_heard / n),
        pct(sum_shipped / n),
        pct(sum_now / n),
        cases.len()
    );
    for (i, want, raw, now) in misses {
        println!("\n#{i} heard: {raw}\n   now:   {now}\n   want:  {want}");
    }
    true
}

/// Runs the polish step exactly as the app does and prints where the time went. Uses the running llama-server.
async fn polishtest(raw: &str) -> bool {
    let Some(mut config) = load_config() else { return false };
    if let Ok(extra) = std::env::var("JEZ_TONES_JSON") {
        if let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, String>>(&extra) {
            config.tones.extend(map);
        }
    }
    if std::env::var("JEZ_FORCE_CLOUD").is_ok() {
        config.cloud_enabled = true;
        config.cloud_dictation = "all".into();
    }
    let tone_names: Vec<String> = std::env::var("JEZ_TONES")
        .map(|t| t.split(',').map(String::from).collect())
        .unwrap_or_else(|_| vec!["casual".into(), "professional".into()]);
    let history = history();
    let polisher = Polisher::new(config.clone(), Scope::Dictation);
    if !polisher.is_healthy().await {
        println!("polishtest: llama-server not running");
        return false;
    }
    let mut examples = Examples::new();
    for name in config.tones.keys() {
        let c: Vec<(String, String)> = history
            .iter()
            .filter(|d| &d.tone == name && d.correction.is_some())
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|d| (d.raw.clone(), d.correction.clone().unwrap()))
            .collect();
        if !c.is_empty() {
            examples.insert(name.clone(), c);
        }
    }
    for tone in &tone_names {
        for pass in 1..=2 {
            let t0 = Instant::now();
            let first: Mutex<Option<Instant>> = Mutex::new(None);
            let on_partial = |_: &str| {
                first.lock().unwrap().get_or_insert_with(Instant::now);
            };
            let out = pipeline::run(raw, tone, &config, Some(&polisher as &dyn PolishBackend), &examples, None, Some(&on_partial))
                .await
                .unwrap_or_else(|_| "(failed)".into());
            let first_ms = first.lock().unwrap().unwrap_or_else(Instant::now).duration_since(t0).as_millis();
            println!(
                "{tone} pass {pass}: {} ms total, first token {first_ms} ms; {}",
                t0.elapsed().as_millis(),
                polisher.last_timings()
            );
            if pass == 2 {
                println!("  -> {out}");
            }
        }
    }
    true
}

/// Records 3 s twice and reports what the microphone delivered.
async fn mictest(args: &[String]) -> bool {
    let p = Platform::native();
    let inputs: Vec<String> = p
        .devices
        .inputs()
        .iter()
        .map(|d| format!("{}{}", d.name, if d.is_built_in { " (built-in)" } else { "" }))
        .collect();
    println!("inputs: {}", inputs.join(", "));
    let pref = if has(args, "--default") || has(args, "--enginetest") { "" } else { "builtin" };
    let seconds: u64 = value(args, "--seconds=").and_then(|s| s.parse().ok()).unwrap_or(3);
    let gap: u64 = value(args, "--gap=").and_then(|s| s.parse().ok()).unwrap_or(0);
    for round in 1..=2 {
        let first: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let f2 = Arc::clone(&first);
        p.mic.set_on_level(Some(Arc::new(move |_| {
            f2.lock().unwrap().get_or_insert_with(Instant::now);
        })));
        let t0 = Instant::now();
        if let Err(e) = p.mic.start(pref) {
            println!("round {round}: start failed: {e:#}");
            continue;
        }
        println!("round {round}: started");
        let mut timeline = vec![];
        for _ in 0..seconds {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let snap = p.mic.snapshot();
            let last = &snap[snap.len().saturating_sub(SAMPLE_RATE)..];
            timeline.push(format!("{:.3}", last.iter().fold(0f32, |m, x| m.max(x.abs()))));
        }
        let s = p.mic.stop();
        let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
        let first_ms = first.lock().unwrap().map(|f| f.duration_since(t0).as_millis() as i64).unwrap_or(-1);
        println!("round {round}: {} samples ({} ms), peak {peak}, first buffer after {first_ms} ms", s.len(), s.len() / 16);
        println!("   per-second peak: {}", timeline.join(" "));
        if round == 1 && gap > 0 {
            println!("   waiting {gap} s (session idles out after 20 s)");
            tokio::time::sleep(Duration::from_secs(gap)).await;
        }
    }
    p.mic.shutdown();
    true
}

/// Runs the meeting notes step on a sample transcript against the running llama-server.
async fn notestest() -> bool {
    let Some(config) = load_config() else { return false };
    let polisher = Polisher::new(config, Scope::Meetings);
    if !polisher.is_healthy().await {
        println!("llama-server not running");
        return false;
    }
    let transcript = "[0:03] Them: thanks everyone for joining two decisions today first we ship the pricing page on thursday second alex owns the instagram reel and will send a draft by friday\n[0:15] You: sounds good i will review the draft on friday morning and i can also book the venue for the launch party by wednesday\n[0:24] Them: great and taylor can you send the investor update out on thursday after the pricing page is live\n[0:31] You: yes i will send it thursday afternoon";
    match polisher.summarize_meeting(transcript).await {
        Ok(summary) => {
            let mut m = Meeting::new("Meeting · test".into(), None, chrono::Utc::now());
            m.refresh_todos(&summary);
            let title = polisher.meeting_title(&summary).await.unwrap_or_default();
            let todos: Vec<&str> = m.todos.iter().map(|t| t.text.as_str()).collect();
            println!("TITLE: {title}\n\n{summary}\n\nTODOS: {todos:?}");
            true
        }
        Err(e) => {
            println!("failed: {e:#}");
            false
        }
    }
}

/// Local vs Groq on the newest N clips: where transcripts and final text differ, with timings.
async fn compare(n: usize) -> bool {
    let Some(mut config) = load_config() else { return false };
    let clips: Vec<_> =
        history().into_iter().filter(|d| d.clip.as_ref().is_some_and(|c| paths::clips_dir().join(c).exists())).take(n).collect();
    let (_sidecar, engine, _live) = match asr(&config, true).await {
        Ok(a) => a,
        Err(e) => {
            println!("compare: {e:#}");
            return false;
        }
    };
    config.cloud_enabled = true;
    config.cloud_dictation = "all".into();
    let Some(cloud) = Cloud::new(&config, Scope::Dictation, false) else {
        println!("compare: no cloud key");
        return false;
    };
    let mut local_cfg = config.clone();
    local_cfg.cloud_enabled = false;
    let local = Polisher::new(local_cfg.clone(), Scope::Dictation);
    if !local.is_healthy().await {
        println!("compare: llama-server not running");
        return false;
    }
    let cloud_polisher = Polisher::new(config.clone(), Scope::Dictation);
    let (mut asr_diff, mut polish_diff) = (0, 0);
    let (mut t_la, mut t_ca, mut t_lp, mut t_cp) = (0.0, 0.0, 0.0, 0.0);
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>();
    for (i, d) in clips.iter().enumerate() {
        let Ok(samples) = wav::read(&paths::clips_dir().join(d.clip.as_ref().unwrap())) else { continue };
        let mut t = Instant::now();
        let lraw = engine.transcribe(&samples, true).await.unwrap_or_default();
        t_la += t.elapsed().as_secs_f64();
        t = Instant::now();
        let craw = cloud.transcribe(&samples, None).await.unwrap_or_else(|_| "(cloud failed)".into());
        t_ca += t.elapsed().as_secs_f64();
        t = Instant::now();
        let lpol = pipeline::run(&lraw, &d.tone, &local_cfg, Some(&local as &dyn PolishBackend), &Examples::new(), None, None)
            .await
            .unwrap_or_default();
        t_lp += t.elapsed().as_secs_f64();
        t = Instant::now();
        let cpol =
            pipeline::run(&craw, &d.tone, &config, Some(&cloud_polisher as &dyn PolishBackend), &Examples::new(), None, None)
                .await
                .unwrap_or_default();
        t_cp += t.elapsed().as_secs_f64();
        let (ad, pd) = (norm(&lraw) != norm(&craw), norm(&lpol) != norm(&cpol));
        asr_diff += ad as usize;
        polish_diff += pd as usize;
        if ad || pd {
            println!("\n#{} [{}] {}s", i + 1, d.tone, d.seconds as i64);
            if ad {
                println!("  heard local: {lraw}\n  heard cloud: {craw}");
            }
            if pd {
                println!("  final local: {}\n  final cloud: {}", lpol.replace('\n', " / "), cpol.replace('\n', " / "));
            }
        }
    }
    let c = clips.len().max(1) as f64;
    println!("\n{} clips. Transcripts differ on {asr_diff}, final text differs on {polish_diff}.", clips.len());
    println!(
        "avg speech-to-text: local {:.0} ms, cloud {:.0} ms | avg polish: local {:.0} ms, cloud {:.0} ms",
        t_la / c * 1000.0,
        t_ca / c * 1000.0,
        t_lp / c * 1000.0,
        t_cp / c * 1000.0
    );
    true
}

async fn tapprocs(name: &str) -> bool {
    match Sidecar::new().tap_processes(name).await {
        Ok(n) => {
            println!("{n} audio process(es) match {name}");
            true
        }
        Err(e) => {
            println!("tapprocs: {e:#}");
            false
        }
    }
}

/// Only the system-audio tap for 8 s while `say` speaks; reports what arrived.
async fn taptest() -> bool {
    let sidecar = Sidecar::new();
    let tap = SidecarSystemAudio(Arc::clone(&sidecar));
    let chunks = Arc::new(AtomicUsize::new(0));
    let samples = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(Mutex::new(0f32));
    let (c2, s2, p2) = (Arc::clone(&chunks), Arc::clone(&samples), Arc::clone(&peak));
    let started = tokio::task::spawn_blocking(move || {
        tap.start(
            "",
            Arc::new(move |c: &[f32]| {
                c2.fetch_add(1, Ordering::SeqCst);
                s2.fetch_add(c.len(), Ordering::SeqCst);
                let m = c.iter().fold(0f32, |m, x| m.max(x.abs()));
                let mut p = p2.lock().unwrap();
                *p = p.max(m);
            }),
        )
        .map(|_| tap)
    })
    .await;
    let tap = match started {
        Ok(Ok(tap)) => tap,
        Ok(Err(e)) => {
            println!("taptest: {e:#}");
            return false;
        }
        Err(e) => {
            println!("taptest: {e}");
            return false;
        }
    };
    let _ = std::process::Command::new("/usr/bin/say")
        .args(["-v", "Samantha", "testing the system audio tap one two three"])
        .spawn();
    tokio::time::sleep(Duration::from_secs(8)).await;
    tokio::task::spawn_blocking(move || tap.stop()).await.ok();
    let (c, n, pk) = (chunks.load(Ordering::SeqCst), samples.load(Ordering::SeqCst), *peak.lock().unwrap());
    println!("taptest: {c} chunks, {} s of audio, peak {pk}", n / SAMPLE_RATE);
    n > 0 && pk > 0.01
}

/// Pushes a 16 kHz WAV through the live-typing path into TextEdit (only while it is frontmost), then swaps in
/// the final text.
async fn livetest(args: &[String], path: &str) -> bool {
    let target = arg_after(args, "--target").unwrap_or_else(|| "com.apple.TextEdit".into());
    let p = Platform::native();
    let front_is_target = || p.field.frontmost_app().is_some_and(|a| a.bundle_id == target);
    let (Some(config), Ok(samples)) = (load_config(), wav::read(std::path::Path::new(path))) else {
        println!("livetest: bad config or wav");
        return false;
    };
    let (_sidecar, _engine, live) = match asr(&config, true).await {
        Ok(a) => a,
        Err(e) => {
            println!("livetest: {e:#}");
            return false;
        }
    };
    if !front_is_target() {
        println!("livetest: {target} is not frontmost, aborting");
        return false;
    }
    let mut typer = LiveTyper::new(Arc::clone(&p.keyboard), Arc::clone(&p.field));
    let context = p.field.capture();
    typer.begin(Some(&context));
    let prefix = if context.needs_leading_space() { " " } else { "" };
    println!(
        "field before cursor: {} chars, leading space: {}",
        context.before.chars().count(),
        context.needs_leading_space()
    );
    live.begin_live().await;
    let (mut updates, mut fed) = (0, 0usize);
    let t0 = Instant::now();
    while fed < samples.len() {
        fed = (fed + 8000).min(samples.len()); // 500 ms of "speech" arrives
        tokio::time::sleep(Duration::from_millis(350)).await;
        if let Ok(text) = live.live_update(&samples[..fed]).await {
            updates += 1;
            if front_is_target() {
                typer.update(&format!("{prefix}{text}")).await;
            }
            println!("  live {:.1}s: {text}", t0.elapsed().as_secs_f64());
        }
    }
    let released = Instant::now();
    let raw = live.live_finish(&samples).await.unwrap_or_default();
    let t1 = Instant::now();
    let polisher = Polisher::new(config.clone(), Scope::Dictation);
    let use_polish = polisher.is_healthy().await;
    let backend = use_polish.then_some(&polisher as &dyn PolishBackend);
    let text = pipeline::run(&raw, "professional", &config, backend, &Examples::new(), Some(&context), None)
        .await
        .unwrap_or_else(|_| raw.clone());
    let t2 = Instant::now();
    if !front_is_target() {
        println!("livetest: {target} lost focus before the swap; not touching the field");
        return false;
    }
    typer.finish(&format!("{prefix}{text}")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!(
        "raw:   {raw}\nfinal: {text}\nupdates: {updates}, typed while speaking: {}s, finish after release: {} ms, polish: {} ms (llm {use_polish})",
        released.duration_since(t0).as_secs(),
        t1.duration_since(released).as_millis(),
        t2.duration_since(t1).as_millis()
    );
    !raw.is_empty()
}
