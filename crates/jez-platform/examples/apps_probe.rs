//! Running apps, app info, browser window titles, mic-in-use and the call detector's view right now.
//! Usage: cargo run -p jez-platform --example apps_probe

#[cfg(target_os = "macos")]
fn main() {
    use std::sync::Arc;
    use std::time::Instant;

    use jez_core::traits::{Apps, AudioDevices, FieldReader};
    use jez_platform::detector::BROWSERS;
    use jez_platform::macos::{AxFieldReader, MacApps, MacAudioDevices};
    use jez_platform::MeetingDetector;

    let apps = Arc::new(MacApps::default());
    let t0 = Instant::now();
    let running = apps.running();
    let first = t0.elapsed();
    let t1 = Instant::now();
    let _ = apps.running();
    println!("running apps: {} (first {first:?}, cached icons {:?})", running.len(), t1.elapsed());
    for a in &running {
        println!("  {:<28} {:<40} icon {} bytes", a.name, a.bundle_id, a.icon.as_ref().map_or(0, |i| i.len()));
    }
    for probe in ["com.apple.Notes", "com.apple.FaceTime", "com.apple.Safari", "com.example.missing"] {
        let info = apps.info(probe);
        println!("info({probe}) = {:?}", info.map(|i| (i.name, i.icon.map(|s| s.len()))));
    }
    for a in running.iter().filter(|a| BROWSERS.contains(&a.bundle_id.as_str())) {
        println!("window titles of {}: {:?}", a.name, apps.window_titles(&a.bundle_id));
    }
    if let Some(icon) = running.first().and_then(|a| a.icon.clone()) {
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.decode(&icon[22..]).unwrap_or_default();
        let dim = |o: usize| png.get(o..o + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
        println!("first icon: {}x{} px", dim(16), dim(20));
    }
    let devices = Arc::new(MacAudioDevices);
    println!("microphone in use: {}", devices.microphone_in_use());
    let field = Arc::new(AxFieldReader);
    println!("frontmost: {:?}", field.frontmost_app().map(|a| a.bundle_id));
    let det = MeetingDetector::new(apps, devices, Arc::new(move || field.frontmost_app()));
    println!("current call: {:?} (bundle {:?})", det.current_call(), det.current_call_bundle());
}

#[cfg(not(target_os = "macos"))]
fn main() {}
