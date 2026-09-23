//! `cargo run -p jez-engines --example asr_smoke -- <16 kHz mono wav> [v2|v3] [vocab term ...]`
//!
//! Loads Parakeet through jezflow-helper (downloading models on first run) and transcribes the clip a few
//! times, printing the latency of each decode.

use std::time::Instant;

use jez_core::traits::AsrEngine;
use jez_engines::sidecar::{Sidecar, SidecarAsr};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let wav = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: asr_smoke <wav> [v2|v3] [terms...]"))?;
    let version = args.next().unwrap_or_else(|| "v2".into());
    let terms: Vec<String> = args.collect();

    let samples = jez_core::wav::read(std::path::Path::new(&wav))?;
    println!(
        "clip: {} samples ({:.2} s)",
        samples.len(),
        samples.len() as f64 / 16_000.0
    );

    let sc = Sidecar::new();
    println!("helper: {:?}", sc.helper());
    let asr = SidecarAsr(sc.clone());

    let t = Instant::now();
    asr.load(&version).await?;
    println!("load {version}: {:.2} s", t.elapsed().as_secs_f64());

    if !terms.is_empty() {
        let t = Instant::now();
        asr.set_vocabulary(&terms).await?;
        println!(
            "setVocabulary({terms:?}): {:.2} s",
            t.elapsed().as_secs_f64()
        );
    }

    for i in 0..4 {
        let t = Instant::now();
        let text = asr.transcribe(&samples, true).await?;
        println!(
            "transcribe #{i}: {:.0} ms  {text:?}",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
    let t = Instant::now();
    let text = asr.transcribe(&samples, false).await?;
    println!(
        "transcribe (no rescore): {:.0} ms  {text:?}",
        t.elapsed().as_secs_f64() * 1000.0
    );
    if std::env::var_os("ASR_SMOKE_SENSE").is_some() {
        for i in 0..2 {
            let t = Instant::now();
            let text = asr.transcribe_multilingual(&samples).await?;
            println!(
                "senseVoice #{i}: {:.0} ms  {text:?}",
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    sc.shutdown();
    Ok(())
}
