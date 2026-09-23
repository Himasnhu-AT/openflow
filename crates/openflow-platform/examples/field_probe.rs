//! Like the Swift `--context`: what the Accessibility reader sees in the frontmost app (sizes only, no field text).
//! Usage: cargo run -p openflow-platform --example field_probe [-- --delay=N]  (N seconds to switch to another app)

#[cfg(target_os = "macos")]
fn main() {
    use openflow_core::traits::FieldReader;
    use openflow_platform::macos::AxFieldReader;

    let delay: u64 = std::env::args()
        .find_map(|a| a.strip_prefix("--delay=").map(str::to_string))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(std::time::Duration::from_secs(delay));
    let reader = AxFieldReader;
    println!("trusted: {}", reader.is_trusted(false));
    println!("frontmost: {:?}", reader.frontmost_app().map(|a| (a.bundle_id, a.name)));
    let c = reader.capture();
    println!("window title: {}", c.window_title);
    println!("before cursor: {} chars", c.before.chars().count());
    println!("after cursor: {} chars", c.after.chars().count());
    println!("cursor: {:?}", c.cursor);
    println!("needs leading space: {}", c.needs_leading_space());
}

#[cfg(not(target_os = "macos"))]
fn main() {}
