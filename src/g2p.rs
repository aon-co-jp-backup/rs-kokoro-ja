//! 日本語の読み変換(G2P): 文章 → Kokoro用の音素列。
//!
//! misaki(`misaki/cutlet.py`、polm/cutlet由来・MIT)の `Cutlet` と同じ規則のRust移植。
//! 形態素解析だけが違う: misakiはMeCab(fugashi)+UniDic、こちらは同系の辞書を使うLindera。
//! 分割が変わる箇所では出力が変わりうる(`tests/` の対照表で、Python版との一致率を測っている)。
//! 表(`data/hepburn.tsv`)と語リスト(`data/ja_words.txt`)はmisakiから書き出したもの。

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

const HEPBURN_TSV: &str = include_str!("../data/hepburn.tsv");
const JA_WORDS_TXT: &str = include_str!("../data/ja_words.txt");

/// MeCabの文字種(char.defの順): 3=記号、6=ひらがな、7=カタカナ など。
const CT_SYMBOL: u8 = 3;
const CT_NORMAL: u8 = 6;

const SUTEGANA: &str = "ゃゅょぁぃぅぇぉ";
const ODORI: &str = "〃々ゝゞヽ";
/// 小書きのカタカナ拡張 → 通常のカタカナ(misakiの `Katakana_Phonetic_Extensions`)。
const KATAKANA_EXT: [(char, char); 16] = [
    ('ㇰ', 'ク'), ('ㇱ', 'シ'), ('ㇲ', 'ス'), ('ㇳ', 'ト'), ('ㇴ', 'ヌ'), ('ㇵ', 'ハ'), ('ㇶ', 'ヒ'), ('ㇷ', 'フ'),
    ('ㇸ', 'ヘ'), ('ㇹ', 'ホ'), ('ㇺ', 'ム'), ('ㇻ', 'ラ'), ('ㇼ', 'リ'), ('ㇽ', 'ル'), ('ㇾ', 'レ'), ('ㇿ', 'ロ'),
];

struct Word {
    surface: String,
    hira: String,
    char_type: u8,
}

struct Tok {
    surface: String,
    space: bool,
}

/// 日本語のG2P。作成は重い(辞書の読み込み)ので、使い回す。`Sync`なので複数スレッドで共有できる。
pub struct JaG2p {
    segmenter: Segmenter,
    table: HashMap<String, String>,
    words: HashSet<&'static str>,
}

fn kata2hira(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            'ヽ' => 'ゝ',
            'ヾ' => 'ゞ',
            _ => c,
        })
        .collect()
}

fn add_dakuten(c: char) -> Option<char> {
    let from: Vec<char> = "かきくけこさしすせそたちつてとはひふへほ".chars().collect();
    let to: Vec<char> = "がぎぐげござじずぜぞだぢづでどばびぶべぼ".chars().collect();
    from.iter().position(|x| *x == c).map(|i| to[i])
}

/// MeCabの文字種の近似(未知語の種別判定用)。
fn mecab_char_type(c: char) -> u8 {
    match c {
        ' ' | '\t' | '\u{3000}' => 1,
        '\u{3041}'..='\u{309F}' => 6,                                    // ひらがな
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => 7, // カタカナ
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}' | '々' | '〆' => 2, // 漢字
        '0'..='9' | '０'..='９' => 4,
        'a'..='z' | 'A'..='Z' | 'ａ'..='ｚ' | 'Ａ'..='Ｚ' => 5,
        '\u{0370}'..='\u{03FF}' => 9,
        '\u{0400}'..='\u{04FF}' => 10,
        c if c.is_ascii_punctuation() => 3,
        '\u{3000}'..='\u{303F}' | '\u{2000}'..='\u{206F}' | '\u{FF00}'..='\u{FF65}' => 3,
        _ => 0,
    }
}

fn is_ascii_alnum_run(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

impl JaG2p {
    pub fn new() -> Result<Self, String> {
        let dictionary = load_dictionary("embedded://unidic").map_err(|e| format!("could not load the UniDic dictionary: {e}"))?;
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        let table = HEPBURN_TSV
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let words = JA_WORDS_TXT.lines().map(str::trim).collect();
        Ok(Self { segmenter, table, words })
    }

    /// 文章 → 音素列(Kokoro v1.0の語彙の文字だけでできている)。空なら空文字。
    pub fn phonemes(&self, text: &str) -> Result<String, String> {
        if text.is_empty() {
            return Ok(String::new());
        }
        let text = normalize_text(text);
        let words = self.words_of(&text)?;
        let tokens = self.romaji_tokens(words);
        let out: String = tokens.iter().map(|t| format!("{}{}", t.surface, if t.space { " " } else { "" })).collect();
        // 空白を1つに畳み、括弧を引用符へ
        let collapsed = out.trim().split_whitespace().collect::<Vec<_>>().join(" ").replace('(', "«").replace(')', "»");
        Ok(fix_glottal_spaces(&collapsed))
    }

    /// 形態素解析して、(表層, ひらがな読み, 文字種)の列にする。
    fn words_of(&self, text: &str) -> Result<Vec<Word>, String> {
        let mut tokens = self.segmenter.segment(Cow::Borrowed(text)).map_err(|e| format!("segmentation failed: {e}"))?;
        let mut words: Vec<Word> = Vec::new();
        for t in tokens.iter_mut() {
            let surface = t.surface.to_string();
            let unknown = t.word_id.is_unknown();
            let details: Vec<String> = t.details().iter().map(|s| s.to_string()).collect();
            // UniDic 2.1.2: 6=lForm(カナ) 9=pron(発音)。pron → kana → 表層 の順(misakiと同じ)
            let pron = details.get(9).filter(|s| !s.is_empty() && s.as_str() != "*").cloned();
            let kana = details.get(6).filter(|s| !s.is_empty() && s.as_str() != "*").cloned();
            let reading = pron.or(kana).unwrap_or_else(|| surface.clone());
            let ct = if unknown {
                let c = surface.chars().next().map(mecab_char_type).unwrap_or(0);
                if c == 7 { CT_NORMAL } else { c }
            } else {
                CT_NORMAL
            };
            words.push(Word { surface, hira: kata2hira(&reading), char_type: ct });
        }
        // MeCabは英数字の連なりを1つの未知語にするが、Linderaは辞書にある1文字ずつに割る。
        // 連続する英数字(ASCII)の語を1つに戻す。
        let mut merged: Vec<Word> = Vec::new();
        for w in words {
            if let Some(last) = merged.last_mut() {
                if is_ascii_alnum_run(&last.surface) && is_ascii_alnum_run(&w.surface) {
                    last.surface.push_str(&w.surface);
                    last.hira = last.surface.clone();
                    last.char_type = 5;
                    continue;
                }
            }
            merged.push(w);
        }
        Ok(merged)
    }

    fn romaji_tokens(&self, words: Vec<Word>) -> Vec<Tok> {
        // 語リスト(ja_words)にある連なりは1語にまとめる(同じ文字種の範囲で、長い方から)
        let mut groups: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let z = (i + 1..words.len()).find(|&z| words[z].char_type != words[i].char_type).unwrap_or(words.len());
            let j = (i + 1..=z).rev().find(|&j| {
                let joined: String = words[i..j].iter().map(|w| w.surface.as_str()).collect();
                self.words.contains(joined.as_str())
            });
            match j {
                None => {
                    groups.push((i, i + 1));
                    i += 1;
                }
                Some(j) => {
                    groups.push((i, j));
                    i = j;
                }
            }
        }
        let merged: Vec<Word> = groups
            .iter()
            .map(|&(a, b)| Word {
                surface: words[a..b].iter().map(|w| w.surface.as_str()).collect(),
                hira: words[a..b].iter().map(|w| w.hira.as_str()).collect(),
                char_type: words[a].char_type,
            })
            .collect();

        let mut out: Vec<Tok> = Vec::new();
        for word in &merged {
            let roma = self.romaji_word(word);
            let mut tok = Tok { surface: roma.clone(), space: false };
            // Pythonの `x in '「『«'` は部分文字列の判定(空文字も真)。同じ意味で `contains` を使う。
            if "「『«".contains(word.surface.as_str()) || "([".contains(roma.as_str()) {
                if let Some(po) = out.last_mut() {
                    po.space = true;
                }
            } else if "」』»".contains(word.surface.as_str()) || "]).,?!:".contains(roma.as_str()) {
                if let Some(po) = out.last_mut() {
                    po.space = false;
                }
                tok.space = true;
            } else if roma == " " {
                tok.space = false;
            } else {
                tok.space = true;
            }
            out.push(tok);
        }
        for t in &mut out {
            t.surface = t.surface.replace('っ', "");
        }
        out
    }

    fn romaji_word(&self, word: &Word) -> String {
        let surface = &word.surface;
        if surface.is_ascii() {
            return surface.clone();
        }
        if word.char_type == CT_SYMBOL {
            return surface.chars().map(|c| self.table.get(&c.to_string()).cloned().unwrap_or_else(|| c.to_string())).collect();
        }
        if word.char_type != CT_NORMAL {
            return String::new();
        }
        let hira: Vec<char> = word.hira.chars().collect();
        let mut out = String::new();
        for (ki, &ch) in hira.iter().enumerate() {
            let nk = hira.get(ki + 1).copied();
            let pk = if ki > 0 { Some(hira[ki - 1]) } else { None };
            out.push_str(&self.single_mapping(pk, ch, nk));
        }
        out
    }

    fn lookup(&self, key: &str) -> Option<&String> {
        self.table.get(key)
    }

    fn single_mapping(&self, pk: Option<char>, kk: char, nk: Option<char>) -> String {
        if ODORI.contains(kk) {
            if "ゝヽ".contains(kk) {
                return pk.map(|c| c.to_string()).unwrap_or_default();
            }
            if "ゞヾ".contains(kk) {
                return match pk.and_then(add_dakuten) {
                    Some(v) => self.lookup(&v.to_string()).cloned().unwrap_or_default(),
                    None => String::new(),
                };
            }
            return String::new();
        }
        if let Some(p) = pk {
            let pair = format!("{p}{kk}");
            if let Some(v) = self.lookup(&pair) {
                return v.clone();
            }
        }
        if let Some(n) = nk {
            let pair = format!("{kk}{n}");
            if self.lookup(&pair).is_some() {
                return String::new();
            }
            if SUTEGANA.contains(n) {
                if kk == 'っ' {
                    return String::new();
                }
                let base = self.lookup(&kk.to_string()).cloned().unwrap_or_default();
                let mut chars: Vec<char> = base.chars().collect();
                chars.pop(); // 末尾(母音)を落として、小さい仮名の母音をつなぐ
                let head: String = chars.into_iter().collect();
                return format!("{head}{}", self.lookup(&n.to_string()).cloned().unwrap_or_default());
            }
        }
        if SUTEGANA.contains(kk) {
            return String::new();
        }
        if kk == 'ー' {
            return "ː".to_string();
        }
        if kk == 'っ' {
            return "ʔ".to_string();
        }
        if kk == 'ん' {
            // 次の音に応じた鼻音(m/ŋ/ɲ/n/ɴ)
            if let Some(tnk) = nk.and_then(|n| self.lookup(&n.to_string())) {
                let first = tnk.chars().next().unwrap_or(' ');
                if "mpb".contains(first) {
                    return "m".into();
                } else if "kɡ".contains(first) {
                    return "ŋ".into();
                } else if ["ɲ", "ʨ", "ʥ"].iter().any(|p| tnk.starts_with(p)) {
                    return "ɲ".into();
                } else if "ntdɾz".contains(first) {
                    return "n".into();
                }
            }
            return "ɴ".into();
        }
        self.lookup(&kk.to_string()).cloned().unwrap_or_default()
    }
}

/// 前処理: 波ダッシュ→「から」、カタカナ拡張、NFKC、数字→ひらがな。
fn normalize_text(text: &str) -> String {
    // 〜/～ の直後が数字なら「から」
    let chars: Vec<char> = text.chars().collect();
    let mut t = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if (c == '〜' || c == '～') && chars.get(i + 1).is_some_and(|n| n.is_numeric() && n.is_ascii_digit() || ('０'..='９').contains(n)) {
            t.push_str("から");
        } else if let Some((_, to)) = KATAKANA_EXT.iter().find(|(k, _)| *k == c) {
            t.push(*to);
        } else {
            t.push(c);
        }
    }
    let t: String = t.nfkc().collect();
    // 数字の連なりを「 +ひらがな」に(数字以外はそのまま)
    let mut out = String::with_capacity(t.len());
    let mut digits = String::new();
    for c in t.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push(' ');
                out.push_str(&crate::num2kana::convert(&digits));
                digits.clear();
            }
            out.push(c);
        }
    }
    if !digits.is_empty() {
        out.push(' ');
        out.push_str(&crate::num2kana::convert(&digits));
    }
    out
}

/// 促音(ʔ)の前後の余計な空白を取る(misakiの正規表現
/// `(?<![!",.:;?»—…”]) (?=ʔ)|(?<=ʔ) (?!["«“])` と同じ)。
fn fix_glottal_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = chars.get(i + 1).copied();
            let before_glottal = next == Some('ʔ') && !prev.is_some_and(|p| "!\",.:;?»—…”".contains(p));
            let after_glottal = prev == Some('ʔ') && !next.is_some_and(|n| "\"«“".contains(n));
            if before_glottal || after_glottal {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glottal_spaces_match_the_regex_rules() {
        assert_eq!(fix_glottal_spaces("a ʔb"), "aʔb");
        assert_eq!(fix_glottal_spaces("aʔ b"), "aʔb");
        assert_eq!(fix_glottal_spaces("a, ʔb"), "a, ʔb"); // 句読点の後ろは残す
        assert_eq!(fix_glottal_spaces("aʔ «b"), "aʔ «b");
    }

    #[test]
    fn kana_helpers() {
        assert_eq!(kata2hira("ハッピョー"), "はっぴょー");
        assert_eq!(add_dakuten('か'), Some('が'));
        assert_eq!(add_dakuten('あ'), None);
    }

    #[test]
    fn normalization_reads_numbers_and_wave_dash() {
        assert_eq!(normalize_text("2026年"), " にせんにじゅうろく年");
        assert!(normalize_text("1〜3").contains("から"));
        assert_eq!(normalize_text("ﾃｽﾄ"), "テスト"); // 半角カナ→全角(NFKC)
    }
}
