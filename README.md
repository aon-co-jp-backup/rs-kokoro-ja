# rs-kokoro-ja

**Kokoro-82M の日本語音声合成を、Rust だけで。** Python も espeak も OpenJTalk のバイナリも要りません。
日本語の文章 → 読み(G2P) → 音素 → ONNX 推論 → 無音トリム → WAV までを 1 つのライブラリ(と CLI)で行います。

*Japanese text-to-speech with [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) in pure Rust — no Python. Text → reading (G2P) → phonemes → ONNX inference → silence trim → WAV, as a library and a CLI.*

- **G2P**: [misaki](https://github.com/hexgrad/misaki)(cutlet 系)と同じ規則の移植。形態素解析だけ MeCab → [Lindera](https://github.com/lindera/lindera)(UniDic 埋め込み)。
  **Python 版の出力と、対照表 41 文で完全一致**(`tests/g2p_parity.rs`)。
- **推論**: [ort](https://github.com/pykeio/ort)(ONNX Runtime)。int8 版(約 92MB)で CPU のみ。
  実測: 4 コア(Ryzen 9 3950X)で 5.6 秒の音声を約 2.7 秒(RTF 約 0.5)。
- **声**: `voices-v1.0.bin`(npz)を直接読む。日本語は `jf_alpha`(女性、最高評価 C+)など 5 種([VOICES.md](https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md))。
- 長い文は 510 トークン以内に自動分割し、句読点に応じた間をあけてつなぐ(kokoro-onnx と同じ)。

## 使い方(CLI)

モデルを用意する(このリポジトリには含めません):

```bash
mkdir model && cd model
curl -LO https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx
curl -LO https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin
```

```bash
cargo run --release --features engine -- --model-dir model --text "おかえりなさいませ、ご主人様。" --out out.wav
# 音素列だけ見る(モデル不要):
cargo run --release --features engine -- --text "今日はいい天気ですね" --phonemes
```

オプション: `--voice jf_alpha|jf_gongitsune|jf_nezumi|jf_tebukuro|jm_kumo`、`--speed 0.5〜2.0`、`--threads N`。

## 使い方(ライブラリ)

```rust
use kokoro_ja::{engine::{wav_bytes, Engine, Vocab, Voices, SAMPLE_RATE}, JaG2p};
use std::path::Path;

let g2p = JaG2p::new()?;                       // 辞書の読み込みは重いので使い回す(Sync)
let voices = Voices::load(Path::new("model/voices-v1.0.bin"))?;
let vocab = Vocab::builtin()?;
let mut engine = Engine::load(Path::new("model/kokoro-v1.0.int8.onnx"), 4)?; // 同時に複数処理するなら、スレッドごとに1つ
let phonemes = g2p.phonemes("世界のニュースをお調べしますわ。")?;
let audio = engine.speak(&vocab, &voices, "jf_alpha", &phonemes, 1.0)?;       // f32, 24kHz
std::fs::write("out.wav", wav_bytes(&audio, SAMPLE_RATE))?;
```

`G2P だけ`なら feature `engine` は不要(ONNX Runtime を使わない)。

## 制限(正直な開示)

- **日本語のみ**。英語など他言語は対象外(Kokoro の英語は別の G2P が要る)。
- 形態素解析が MeCab と Lindera で違うため、対照表の外の文では Python 版と読みが変わりうる。
  分割が違う箇所(たとえば英字の連なり)は、既知の差を吸収する処理を入れてある。10 桁以上の数は 1 桁ずつ読む(元の実装はエラー文を返す)。
- 音声そのものは、Python 版と同じ長さ・同じ音量になることを確認したが、波形は一致しない(ONNX の乱数を使う層があるため)。
  聴感品質はモデル(Kokoro)の水準で、日本語は話者の評価が高くない(C+ 以下)。
- ビルドは ONNX Runtime を自動で取得する(`ort` の既定)。初回は時間がかかる。辞書(UniDic)をバイナリへ埋め込むため、バイナリは大きい。

## 開発

```bash
cargo test --release                                   # G2P(対照表)・単体試験
KOKORO_MODEL_DIR=model cargo test --release --features engine   # 実モデルの通し試験
```

対照表(`tests/expected_phonemes.tsv`)と表データは、misaki から `tools/` のスクリプトで書き出したもの(開発時の照合用。実行時には Python を使わない)。

## ライセンス

Apache-2.0([LICENSE](LICENSE))。取り込んだ成果とモデルの権利は [NOTICE](NOTICE) を参照。
Kokoro-82M の重みは Apache-2.0。学習データの注記(許諾のある音声と、大手 TTS の合成音声を含む)はモデルカードを参照してください。
