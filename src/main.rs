//! `kokoro-ja`: 日本語の文章をKokoroでWAVにする(Pythonなし)。
//!
//! 使い方: kokoro-ja --model-dir <フォルダ> --text "こんにちは" --out out.wav [--voice jf_alpha] [--speed 1.0] [--threads 4]
//! フォルダには kokoro-v1.0.int8.onnx(または kokoro-v1.0.onnx)と voices-v1.0.bin が要る。
//! `--phonemes` を付けると、音声は作らず音素列だけを表示する(モデル不要)。

use kokoro_ja::engine::{wav_bytes, Engine, Vocab, Voices, SAMPLE_RATE};
use kokoro_ja::JaG2p;
use std::path::PathBuf;
use std::time::Instant;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let text = arg(args, "--text").ok_or("--text が必要です")?;
    let g2p = JaG2p::new()?;
    let ph = g2p.phonemes(&text)?;
    if args.iter().any(|a| a == "--phonemes") {
        println!("{ph}");
        return Ok(());
    }
    let dir = PathBuf::from(arg(args, "--model-dir").ok_or("--model-dir が必要です")?);
    let model = ["kokoro-v1.0.int8.onnx", "kokoro-v1.0.onnx"].iter().map(|n| dir.join(n)).find(|p| p.is_file()).ok_or("モデル(kokoro-v1.0.int8.onnx)が見つかりません")?;
    let voice = arg(args, "--voice").unwrap_or_else(|| "jf_alpha".into());
    let speed: f32 = arg(args, "--speed").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let threads: usize = arg(args, "--threads").and_then(|s| s.parse().ok()).unwrap_or(4);
    let out = arg(args, "--out").ok_or("--out が必要です")?;
    let voices = Voices::load(&dir.join("voices-v1.0.bin"))?;
    let vocab = Vocab::builtin()?;
    let mut engine = Engine::load(&model, threads)?;
    let t = Instant::now();
    let audio = engine.speak(&vocab, &voices, &voice, &ph, speed)?;
    let secs = audio.len() as f32 / SAMPLE_RATE as f32;
    std::fs::write(&out, wav_bytes(&audio, SAMPLE_RATE)).map_err(|e| e.to_string())?;
    println!("{out}: {secs:.2}s の音声を {:.2}s で合成(RTF {:.2})", t.elapsed().as_secs_f32(), t.elapsed().as_secs_f32() / secs);
    Ok(())
}
