//! Kokoro-82M(ONNX)の日本語音声合成をRustだけで行う。
//!
//! - [`g2p`]: 日本語の文章 → Kokoroの音素列(misakiのcutletと同じ規則)
//! - `engine`(feature `engine`): 音素 → トークン化 → ONNX推論 → 無音トリム → WAV
//!
//! Pythonは使わない。

pub mod g2p;
pub mod num2kana;
#[cfg(feature = "engine")]
pub mod engine;

pub use g2p::JaG2p;
