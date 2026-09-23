//! Drives the LiveTyper against the real focused field (like the Swift `--livetest`): live partials, final swap,
//! then a retract round. Refuses to run unless TextEdit is frontmost, since it types into the focused field.
//! Usage: open -a TextEdit <new file>; cargo run -p openflow-platform --example typer_probe

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use std::time::Duration;

    use openflow_core::traits::{FieldReader, Keyboard};
    use openflow_platform::macos::{AxFieldReader, MacKeyboard};
    use openflow_platform::LiveTyper;

    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    let field = Arc::new(AxFieldReader);
    let keyboard = Arc::new(MacKeyboard);
    let front = field.frontmost_app().map(|a| a.bundle_id).unwrap_or_default();
    if front != "com.apple.TextEdit" {
        println!("frontmost is {front:?}, not TextEdit; refusing to type");
        std::process::exit(2);
    }
    let clip_before = keyboard.clipboard_get();
    let mut typer = LiveTyper::new(keyboard.clone(), field.clone());

    let ctx = field.capture();
    println!("start: cursor {:?}, before {:?}", ctx.cursor, ctx.before);
    typer.begin(Some(&ctx));
    for partial in ["hello", "hello wor", "hello world this", "hello world this is a", "hello world, this is a test."] {
        typer.update(partial).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    println!("live: {:?} desynced {}", field.capture().before, typer.is_desynced());
    let final_text = "Hello world, this is a test.";
    typer.finish(final_text).await;
    let after = field.capture();
    println!("final: {:?}", after.before);
    let ok1 = after.before.ends_with(&format!("{}{final_text}", ctx.before));
    println!("finish ok: {ok1}");

    let ctx = field.capture();
    typer.begin(Some(&ctx));
    typer.update(" some live words").await;
    typer.update(" some live words to cancel").await;
    typer.retract().await;
    let after = field.capture();
    let ok2 = after.before == ctx.before;
    println!("retract ok: {ok2} ({:?})", after.before);
    println!("clipboard restored: {} (had text before: {})", keyboard.clipboard_get() == clip_before, clip_before.is_some());
    std::process::exit(if ok1 && ok2 { 0 } else { 1 });
}

#[cfg(not(target_os = "macos"))]
fn main() {}
