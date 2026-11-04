# rs-kokoro-ja

**Kokoro-82M の日本語音声合成を、Rust だけで。** Python も espeak も OpenJTalk のバイナリも要りません。
日本語の文章 → 読み(G2P) → 音素 → ONNX 推論 → 無音トリム → WAV までを 1 つのライブラリ(と CLI)で行います。

*Japanese text-to-speech with [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) in pure Rust — no Python. Text → reading (G2P) → phonemes → ONNX inference → silence trim → WAV, as a library and a CLI.*

- **G2P**: [misaki](https://github.com/hexgrad/misaki)(cutlet 系)と同じ規則の移植。形態素解析だけ MeCab → [Lindera](https://github.com/lindera/lindera)(UniDic 埋め込み)。
  **Python 版の出力と、対照表 41 文で完全一致**(`tests/g2p_parity.rs`)。
- **推論**: [ort](https://github.com/pykeio/ort)(ONNX Runtime)。int8 版(約 92MB)で CPU のみ。
  実測: 4 スレッド・int8・CPU のみで、5.6 秒の音声を Ryzen 9 3950X で約 2.7 秒(RTF 0.49)、ConoHa VPS の Xeon(Ice Lake)4 コアで約 2.2 秒(RTF 0.39、2 スレッドなら 0.63)。
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

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.
