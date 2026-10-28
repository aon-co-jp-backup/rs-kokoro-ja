//! 音素列 → 音声(Kokoro-82M ONNX)。語彙・声(npz)・分割・推論・無音トリム・WAV。
//!
//! kokoro-onnx(Python、MIT)の `create()` と同じ手順の移植:
//! 音素列を510トークン以内の塊に分け(文→句→語の順に切る)、塊ごとに
//! `[0, ..トークン.., 0]` と声のスタイル(`voice[トークン数-1]`)をモデルへ入れ、
//! 先頭と末尾の無音を取り、句読点に応じた間(文0.25秒・句0.1秒)をあけてつなぐ。

use ndarray::Array1;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub const SAMPLE_RATE: u32 = 24000;
const MAX_PHONEME_LENGTH: usize = 510;
const STYLE_DIM: usize = 256;
const SENTENCE_PAUSE: f32 = 0.25;
const CLAUSE_PAUSE: f32 = 0.1;
const CONFIG_JSON: &str = include_str!("../data/kokoro_config.json");

fn ort_err(e: impl std::fmt::Display) -> String {
    format!("onnxruntime: {e}")
}

/// 音素の文字 → トークンID(モデルが学習した語彙)。
pub struct Vocab(HashMap<char, i64>);

impl Vocab {
    pub fn builtin() -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(CONFIG_JSON).map_err(|e| e.to_string())?;
        let map = v.get("vocab").and_then(|x| x.as_object()).ok_or("no vocab in config")?;
        let mut out = HashMap::new();
        for (k, id) in map {
            let mut cs = k.chars();
            if let (Some(c), None) = (cs.next(), cs.next()) {
                out.insert(c, id.as_i64().ok_or("bad vocab id")?);
            }
        }
        Ok(Self(out))
    }
    /// 語彙にある文字だけをトークンIDにする(無い文字は捨てる)。
    pub fn tokenize(&self, phonemes: &str) -> Vec<i64> {
        phonemes.chars().filter_map(|c| self.0.get(&c).copied()).take(MAX_PHONEME_LENGTH).collect()
    }
    pub fn filter(&self, phonemes: &str) -> String {
        phonemes.chars().filter(|c| self.0.contains_key(c)).collect()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 声の一覧(`voices-v1.0.bin` = npz: 声名ごとの `(510, 1, 256)` のf32配列)。
pub struct Voices {
    map: HashMap<String, Vec<f32>>, // 510*256 個
}

fn parse_npy(bytes: &[u8]) -> Result<(Vec<usize>, Vec<f32>), String> {
    if bytes.len() < 10 || &bytes[..6] != b"\x93NUMPY" {
        return Err("not an npy file".into());
    }
    let major = bytes[6];
    let (hlen, start) = if major == 1 {
        (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10)
    } else {
        (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12)
    };
    let header = std::str::from_utf8(&bytes[start..start + hlen]).map_err(|e| e.to_string())?;
    if !header.contains("'<f4'") && !header.contains("\"<f4\"") {
        return Err(format!("unsupported dtype in npy header: {header}"));
    }
    let shape_src = header.split("'shape':").nth(1).ok_or("no shape")?;
    let inner = shape_src.split('(').nth(1).and_then(|s| s.split(')').next()).ok_or("bad shape")?;
    let shape: Vec<usize> = inner.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let data = &bytes[start + hlen..];
    let n: usize = shape.iter().product();
    if data.len() < n * 4 {
        return Err("npy data is shorter than its shape".into());
    }
    let floats = data[..n * 4].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    Ok((shape, floats))
}

impl Voices {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("voices file is not an npz: {e}"))?;
        let mut map = HashMap::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().trim_end_matches(".npy").to_string();
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            let (shape, data) = parse_npy(&buf).map_err(|e| format!("{name}: {e}"))?;
            if shape.len() != 3 || shape[2] != STYLE_DIM || shape[0] == 0 {
                return Err(format!("{name}: unexpected voice shape {shape:?}"));
            }
            map.insert(name, data);
        }
        Ok(Self { map })
    }
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.map.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }
    /// トークン数 `n` のときのスタイルベクトル(行 `min(n, 行数) - 1`)。
    fn style(&self, name: &str, n: usize) -> Result<&[f32], String> {
        let data = self.map.get(name).ok_or_else(|| format!("voice {name} not found"))?;
        let rows = data.len() / STYLE_DIM;
        let row = n.min(rows).max(1) - 1;
        Ok(&data[row * STYLE_DIM..(row + 1) * STYLE_DIM])
    }
}

// ── 音素列の分割(kokoro-onnxの chunker.py と同じ) ──

const SENTENCE_MARKS: &str = ".!?…";
const CLAUSE_MARKS: &str = ",;:";

fn atoms(ph: &str, max: usize, level: usize, out: &mut Vec<String>) {
    let n = ph.chars().count();
    if n <= max {
        if !ph.is_empty() {
            out.push(ph.to_string());
        }
        return;
    }
    // 切れ目: 0=文末記号の後の空白、1=句の記号の後の空白、2=空白
    for idx in level..3 {
        let pieces = split_boundary(ph, idx);
        if pieces.len() > 1 {
            for p in pieces {
                atoms(p.trim(), max, idx + 1, out);
            }
            return;
        }
    }
    let chars: Vec<char> = ph.chars().collect();
    for chunk in chars.chunks(max) {
        out.push(chunk.iter().collect());
    }
}

fn split_boundary(s: &str, idx: usize) -> Vec<String> {
    let marks = match idx {
        0 => Some(SENTENCE_MARKS),
        1 => Some(CLAUSE_MARKS),
        _ => None,
    };
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let ok = match marks {
                Some(m) => cur.chars().last().is_some_and(|p| m.contains(p)),
                None => true,
            };
            if ok {
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                out.push(std::mem::take(&mut cur));
                continue;
            }
        }
        cur.push(c);
        i += 1;
    }
    out.push(cur);
    out
}

fn pack(lengths: &[usize], limit: usize) -> Vec<(usize, usize)> {
    let mut batches = Vec::new();
    let (mut start, mut size) = (0usize, 0usize);
    for (i, &len) in lengths.iter().enumerate() {
        let cand = if i == start { len } else { size + 1 + len };
        if cand > limit && i > start {
            batches.push((start, i));
            start = i;
            size = len;
        } else {
            size = cand;
        }
    }
    if !lengths.is_empty() {
        batches.push((start, lengths.len()));
    }
    batches
}

/// 音素列を、510以内の塊へ均等に分ける。
pub fn split_phonemes(phonemes: &str, max: usize) -> Vec<String> {
    let mut a = Vec::new();
    atoms(phonemes.trim(), max, 0, &mut a);
    if a.is_empty() {
        return Vec::new();
    }
    let lengths: Vec<usize> = a.iter().map(|x| x.chars().count()).collect();
    let fewest = pack(&lengths, max).len();
    let (mut low, mut high) = (*lengths.iter().max().unwrap_or(&1), max);
    while low < high {
        let mid = (low + high) / 2;
        if pack(&lengths, mid).len() <= fewest {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    pack(&lengths, low).into_iter().map(|(s, e)| a[s..e].join(" ")).collect()
}

fn pause_after(ph: &str) -> f32 {
    match ph.trim_end().chars().last() {
        Some(c) if SENTENCE_MARKS.contains(c) => SENTENCE_PAUSE,
        Some(c) if CLAUSE_MARKS.contains(c) => CLAUSE_PAUSE,
        _ => 0.0,
    }
}

// ── 無音トリム(librosa.effects.trim: top_db=60、frame 2048、hop 512、基準は最大RMS) ──

fn trim_silence(y: &[f32]) -> &[f32] {
    const FRAME: usize = 2048;
    const HOP: usize = 512;
    if y.is_empty() {
        return y;
    }
    let pad = FRAME / 2;
    let mut padded = vec![0.0f32; pad];
    padded.extend_from_slice(y);
    padded.extend(std::iter::repeat(0.0).take(pad));
    if padded.len() < FRAME {
        return y;
    }
    let frames = (padded.len() - FRAME) / HOP + 1;
    let rms: Vec<f32> = (0..frames)
        .map(|f| {
            let s = &padded[f * HOP..f * HOP + FRAME];
            (s.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt()
        })
        .collect();
    let max = rms.iter().cloned().fold(0.0f32, f32::max);
    let db = |v: f32| 20.0 * v.max(1e-5).log10() - 20.0 * max.max(1e-5).log10();
    let non: Vec<usize> = (0..frames).filter(|&f| db(rms[f]) > -60.0).collect();
    match (non.first(), non.last()) {
        (Some(&a), Some(&b)) => &y[a * HOP..((b + 1) * HOP).min(y.len())],
        _ => &y[0..0],
    }
}

/// 16bit・モノラルのWAVにする。
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

fn environment() -> Result<&'static ort::environment::Environment, String> {
    static ENV: std::sync::OnceLock<Result<ort::environment::Environment, String>> = std::sync::OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("kokoro-ja").build().map_err(ort_err)).as_ref().map_err(|e| e.clone())
}

/// ONNXセッション1つ分の合成器。同時に複数処理したいときは、スレッドごとに1つ作る
/// (`Voices`・`Vocab`は共有してよい)。
pub struct Engine {
    session: Session,
    tokens_name: String,
}

impl Engine {
    /// `threads` はこのセッションのONNXスレッド数。
    pub fn load(model: &Path, threads: usize) -> Result<Self, String> {
        let env = environment()?;
        let session = Session::builder(env)
            .map_err(ort_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err)?
            .with_intra_threads(threads.max(1))
            .map_err(ort_err)?
            .commit_from_file(model)
            .map_err(ort_err)?;
        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let tokens_name = if names.iter().any(|n| n == "input_ids") { "input_ids" } else { "tokens" }.to_string();
        Ok(Self { session, tokens_name })
    }

    fn infer(&mut self, tokens: &[i64], style: &[f32], speed: f32) -> Result<Vec<f32>, String> {
        let mut ids = Vec::with_capacity(tokens.len() + 2);
        ids.push(0i64);
        ids.extend_from_slice(tokens);
        ids.push(0);
        let n = ids.len();
        let tokens_t = Tensor::from_array(([1usize, n], ids)).map_err(ort_err)?;
        let style_t = Tensor::from_array(([1usize, STYLE_DIM], style.to_vec())).map_err(ort_err)?;
        let speed_t = Tensor::from_array(([1usize], vec![speed])).map_err(ort_err)?;
        let name = self.tokens_name.clone();
        let outputs = self
            .session
            .run(ort::inputs![name.as_str() => tokens_t, "style" => style_t, "speed" => speed_t])
            .map_err(ort_err)?;
        let (_shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(ort_err)?;
        Ok(data.to_vec())
    }

    /// 音素列を音声(f32、24kHz)にする。`speed` は0.5〜2.0。
    pub fn speak(&mut self, vocab: &Vocab, voices: &Voices, voice: &str, phonemes: &str, speed: f32) -> Result<Vec<f32>, String> {
        if !(0.5..=2.0).contains(&speed) {
            return Err(format!("speed must be between 0.5 and 2.0, got {speed}"));
        }
        let phonemes = phonemes.split_whitespace().collect::<Vec<_>>().join(" ");
        let batches = split_phonemes(&vocab.filter(&phonemes), MAX_PHONEME_LENGTH);
        if batches.is_empty() {
            return Err("nothing to synthesize".into());
        }
        let mut audio: Vec<f32> = Vec::new();
        let last = batches.len() - 1;
        for (i, batch) in batches.iter().enumerate() {
            let tokens = vocab.tokenize(batch);
            if tokens.is_empty() {
                continue;
            }
            let style = voices.style(voice, tokens.len())?;
            let raw = self.infer(&tokens, style, speed)?;
            audio.extend_from_slice(trim_silence(&raw));
            if i != last {
                let pause = pause_after(batch);
                audio.extend(std::iter::repeat(0.0).take((pause * SAMPLE_RATE as f32) as usize));
            }
        }
        if audio.is_empty() {
            return Err("the model produced no audio".into());
        }
        Ok(audio)
    }
}

#[allow(dead_code)]
fn _unused(_: Array1<f32>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_vocab_has_114_symbols_and_filters() {
        let v = Vocab::builtin().unwrap();
        assert_eq!(v.len(), 114);
        assert_eq!(v.tokenize("a❓b").len(), 2); // 語彙に無い記号は捨てる
        assert_eq!(v.filter("kɯ❓"), "kɯ");
    }

    #[test]
    fn split_keeps_short_text_whole_and_cuts_long_text_at_sentences() {
        assert_eq!(split_phonemes("a b c", 510), vec!["a b c".to_string()]);
        let s = format!("{}. {}.", "a".repeat(300), "b".repeat(300));
        let parts = split_phonemes(&s, 510);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.chars().count() <= 510));
        assert!(parts[0].ends_with('.'));
        // 空白の無い巨大な連なりも、落とさずに切る
        let big = "x".repeat(1200);
        let p = split_phonemes(&big, 510);
        assert_eq!(p.iter().map(|x| x.len()).sum::<usize>(), 1200);
    }

    #[test]
    fn pauses_follow_punctuation() {
        assert_eq!(pause_after("abc."), SENTENCE_PAUSE);
        assert_eq!(pause_after("abc,"), CLAUSE_PAUSE);
        assert_eq!(pause_after("abc"), 0.0);
    }

    #[test]
    fn trim_removes_leading_and_trailing_silence() {
        let mut y = vec![0.0f32; 8000];
        y.extend((0..4000).map(|i| (i as f32 * 0.05).sin() * 0.5));
        y.extend(vec![0.0f32; 8000]);
        let t = trim_silence(&y);
        assert!(t.len() < y.len() - 10000 && t.len() > 3500, "{}", t.len());
        // 全部無音なら、librosaと同じく基準が無いので切らない
        assert_eq!(trim_silence(&[0.0; 100]).len(), 100);
    }

    #[test]
    fn wav_header_is_valid() {
        let w = wav_bytes(&[0.0, 0.5, -0.5], 24000);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 6);
        assert_eq!(w.len(), 44 + 6);
    }
}
