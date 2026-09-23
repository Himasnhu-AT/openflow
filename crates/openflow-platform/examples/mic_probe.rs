//! Like the Swift `--mictest`: lists inputs, records 3 s twice (the second round reuses the warm stream) and
//! reports what the microphone delivered.
//! Usage: cargo run -p openflow-platform --example mic_probe [-- <device pref: "" | builtin | UID>] [--seconds=N] [--gap=N]

#[cfg(target_os = "macos")]
fn main() {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use openflow_core::traits::{AudioDevices, Mic};
    use openflow_platform::macos::{MacAudioDevices, MacMic};

    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pref = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let seconds: u64 =
        args.iter().find_map(|a| a.strip_prefix("--seconds=")).and_then(|s| s.parse().ok()).unwrap_or(3);
    let gap: u64 = args.iter().find_map(|a| a.strip_prefix("--gap=")).and_then(|s| s.parse().ok()).unwrap_or(0);

    let devices = MacAudioDevices;
    for d in devices.inputs() {
        println!(
            "input: {} [{}]{}{}",
            d.name,
            d.uid,
            if d.is_built_in { " (built-in)" } else { "" },
            if d.is_continuity { " (continuity)" } else { "" }
        );
    }
    println!("lid closed: {}, mic in use: {}", devices.lid_closed(), devices.microphone_in_use());
    println!("resolve({pref:?}) = {:?}", devices.resolve(&pref).map(|d| d.name));

    let mic = MacMic::new();
    let chunks = Arc::new(Mutex::new(0usize));
    let c2 = Arc::clone(&chunks);
    mic.set_on_chunk(Some(Arc::new(move |_| *c2.lock().unwrap() += 1)));
    for round in 1..=2 {
        let first: Arc<Mutex<Option<Instant>>> = Arc::default();
        let f2 = Arc::clone(&first);
        mic.set_on_level(Some(Arc::new(move |_| {
            f2.lock().unwrap().get_or_insert_with(Instant::now);
        })));
        let t0 = Instant::now();
        if let Err(e) = mic.start(&pref) {
            println!("round {round}: start failed: {e}");
            continue;
        }
        println!("round {round}: started in {} ms", t0.elapsed().as_millis());
        let mut timeline = Vec::new();
        for _ in 0..seconds {
            std::thread::sleep(Duration::from_secs(1));
            let snap = mic.snapshot();
            let peak = snap[snap.len().saturating_sub(16_000)..].iter().fold(0f32, |m, x| m.max(x.abs()));
            timeline.push(format!("{peak:.3}"));
        }
        let s = mic.stop();
        let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
        let first_ms = first.lock().unwrap().map(|f| f.duration_since(t0).as_millis() as i64).unwrap_or(-1);
        println!(
            "round {round}: {} samples ({} ms), peak {peak}, first buffer after {first_ms} ms, device {:?}, running {}",
            s.len(),
            s.len() / 16,
            mic.current_device_uid(),
            mic.is_running()
        );
        println!("   per-second peak: {}", timeline.join(" "));
        println!("   mic in use (warm): {}", devices.microphone_in_use());
        if round == 1 && gap > 0 {
            println!("   waiting {gap} s (stream idles out after 20 s)");
            std::thread::sleep(Duration::from_secs(gap));
            println!("   running after gap: {}, mic in use: {}", mic.is_running(), devices.microphone_in_use());
        }
    }
    println!("chunks delivered (incl. while not capturing): {}", chunks.lock().unwrap());
    mic.shutdown();
    std::thread::sleep(Duration::from_millis(300));
    println!("after shutdown running: {}", mic.is_running());
}

#[cfg(not(target_os = "macos"))]
fn main() {}
