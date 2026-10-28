//! Python(misaki 0.7+のcutlet)が出した音素列(`expected_phonemes.tsv`)との一致を調べる。
//! 形態素解析がMeCab+UniDicとLindera+UniDicで違う箇所だけ差が出る。一致率の下限を守る。
use kokoro_ja::JaG2p;

#[test]
fn matches_the_python_g2p_on_the_corpus() {
    let g = JaG2p::new().expect("dictionary");
    let table = std::fs::read_to_string("tests/expected_phonemes.tsv").unwrap();
    let (mut total, mut same) = (0usize, 0usize);
    let mut diffs = Vec::new();
    for line in table.lines() {
        let Some((text, want)) = line.split_once('\t') else { continue };
        let got = g.phonemes(text).unwrap();
        total += 1;
        if got == want {
            same += 1;
        } else {
            diffs.push(format!("TEXT: {text}\n  py : {want}\n  rs : {got}"));
        }
    }
    for d in &diffs {
        println!("{d}");
    }
    println!("一致 {same}/{total}");
    let min = std::env::var("PARITY_MIN").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(total);
    assert!(same >= min, "一致 {same}/{total}(下限 {min})");
}
