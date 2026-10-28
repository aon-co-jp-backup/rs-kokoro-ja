//! 実モデルでの通し試験。`KOKORO_MODEL_DIR`(kokoro-v1.0.int8.onnx と voices-v1.0.bin のあるフォルダ)が
//! 無ければ何もせず通る(CIではモデルを取得して設定する)。
#![cfg(feature = "engine")]
use kokoro_ja::engine::{wav_bytes, Engine, Vocab, Voices, SAMPLE_RATE};
use kokoro_ja::JaG2p;
use std::path::PathBuf;

fn model_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var("KOKORO_MODEL_DIR").ok()?);
    d.join("kokoro-v1.0.int8.onnx").is_file().then_some(d)
}

#[test]
fn synthesizes_japanese_speech_end_to_end() {
    let Some(dir) = model_dir() else {
        eprintln!("KOKORO_MODEL_DIR が無いのでスキップ");
        return;
    };
    let g2p = JaG2p::new().unwrap();
    let voices = Voices::load(&dir.join("voices-v1.0.bin")).unwrap();
    assert!(voices.names().contains(&"jf_alpha") && voices.names().contains(&"jm_kumo"));
    let vocab = Vocab::builtin().unwrap();
    let mut engine = Engine::load(&dir.join("kokoro-v1.0.int8.onnx"), 2).unwrap();
    let ph = g2p.phonemes("おかえりなさいませ、ご主人様。今日も一緒に英会話のレッスンをしましょうね。").unwrap();
    let audio = engine.speak(&vocab, &voices, "jf_alpha", &ph, 1.0).unwrap();
    let secs = audio.len() as f32 / SAMPLE_RATE as f32;
    assert!((4.5..7.5).contains(&secs), "音声の長さが想定外: {secs}s");
    let rms = (audio.iter().map(|x| x * x).sum::<f32>() / audio.len() as f32).sqrt();
    assert!(rms > 0.02 && rms < 0.5, "音量が想定外: {rms}");
    // 同じ入力は同じ結果(決定的)
    let again = engine.speak(&vocab, &voices, "jf_alpha", &ph, 1.0).unwrap();
    assert_eq!(audio.len(), again.len());
    // 話速: 速くすると短くなる
    let fast = engine.speak(&vocab, &voices, "jf_alpha", &ph, 1.5).unwrap();
    assert!(fast.len() < audio.len());
    // 範囲外の話速と、読めない入力は、パニックせずエラー
    assert!(engine.speak(&vocab, &voices, "jf_alpha", &ph, 3.0).is_err());
    assert!(engine.speak(&vocab, &voices, "jf_alpha", "❓❓", 1.0).is_err());
    assert!(engine.speak(&vocab, &voices, "no_such_voice", &ph, 1.0).is_err());
    let wav = wav_bytes(&audio, SAMPLE_RATE);
    assert_eq!(&wav[..4], b"RIFF");
}

#[test]
fn long_text_is_split_into_batches_and_still_speaks() {
    let Some(dir) = model_dir() else { return };
    let g2p = JaG2p::new().unwrap();
    let voices = Voices::load(&dir.join("voices-v1.0.bin")).unwrap();
    let vocab = Vocab::builtin().unwrap();
    let mut engine = Engine::load(&dir.join("kokoro-v1.0.int8.onnx"), 2).unwrap();
    let text = "本日、各国の中央銀行は、金利の据え置きを相次いで発表しました。".repeat(12);
    let ph = g2p.phonemes(&text).unwrap();
    assert!(ph.chars().count() > 510, "510を超える長さで試す");
    let audio = engine.speak(&vocab, &voices, "jf_alpha", &ph, 1.0).unwrap();
    assert!(audio.len() as f32 / SAMPLE_RATE as f32 > 30.0);
}
