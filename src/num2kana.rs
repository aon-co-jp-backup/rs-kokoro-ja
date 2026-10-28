//! 算用数字 → ひらがな(misaki の `num2kana.Convert(.., 'hiragana')` の移植、9桁まで)。
//!
//! 元: <https://github.com/Greatdane/Convert-Numbers-to-Japanese>(misakiが取り込んだもの)。
//! 小数点・漢数字は扱わない(G2Pの前処理は数字だけの並びにしか使わない)。10桁以上は、元の実装が
//! 英語のエラー文を返してしまうため、**1桁ずつ読む**ことにした(元と異なる点)。

fn d(key: &str) -> &'static str {
    match key {
        "0" => "ゼロ",
        "1" => "いち",
        "2" => "に",
        "3" => "さん",
        "4" => "よん",
        "5" => "ご",
        "6" => "ろく",
        "7" => "なな",
        "8" => "はち",
        "9" => "きゅう",
        "10" => "じゅう",
        "100" => "ひゃく",
        "1000" => "せん",
        "10000" => "まん",
        "100000000" => "おく",
        "300" => "さんびゃく",
        "600" => "ろっぴゃく",
        "800" => "はっぴゃく",
        "3000" => "さんぜん",
        "8000" => "はっせん",
        "01000" => "いっせん",
        _ => "",
    }
}

fn len_one(s: &str) -> String {
    d(s).to_string()
}

fn len_two(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    if c[0] == '0' {
        return len_one(&c[1].to_string());
    }
    if s == "10" {
        return d("10").to_string();
    }
    if c[0] == '1' {
        return format!("{} {}", d("10"), len_one(&c[1].to_string()));
    }
    if c[1] == '0' {
        return format!("{} {}", len_one(&c[0].to_string()), d("10"));
    }
    format!("{} {} {}", d(&c[0].to_string()), d("10"), d(&c[1].to_string()))
}

fn len_three(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut list: Vec<String> = Vec::new();
    match c[0] {
        '1' => list.push(d("100").into()),
        '3' => list.push(d("300").into()),
        '6' => list.push(d("600").into()),
        '8' => list.push(d("800").into()),
        other => {
            list.push(d(&other.to_string()).into());
            list.push(d("100").into());
        }
    }
    if &s[1..] == "00" && s.len() == 3 {
        // 何も足さない
    } else if c[1] == '0' {
        list.push(d(&c[2].to_string()).into());
    } else {
        list.push(len_two(&s[1..]));
    }
    list.join(" ")
}

fn len_four(s: &str, stand_alone: bool) -> String {
    if s == "0000" {
        return String::new();
    }
    let mut s = s.to_string();
    while s.starts_with('0') {
        s.remove(0);
    }
    match s.len() {
        1 => return len_one(&s),
        2 => return len_two(&s),
        3 => return len_three(&s),
        _ => {}
    }
    let c: Vec<char> = s.chars().collect();
    let mut list: Vec<String> = Vec::new();
    match c[0] {
        '1' if stand_alone => list.push(d("1000").into()),
        '1' => list.push(d("01000").into()),
        '3' => list.push(d("3000").into()),
        '8' => list.push(d("8000").into()),
        other => {
            list.push(d(&other.to_string()).into());
            list.push(d("1000").into());
        }
    }
    if &s[1..] == "000" && s.len() == 4 {
        // 何も足さない
    } else if c[1] == '0' {
        list.push(len_two(&s[2..]));
    } else {
        list.push(len_three(&s[1..]));
    }
    list.join(" ")
}

fn len_x(s: &str) -> String {
    let head = &s[..s.len() - 4];
    let mut list: Vec<String> = Vec::new();
    match head.len() {
        1 => {
            list.push(d(head).into());
            list.push(d("10000").into());
        }
        2 => {
            list.push(len_two(&s[0..2]));
            list.push(d("10000").into());
        }
        3 => {
            list.push(len_three(&s[0..3]));
            list.push(d("10000").into());
        }
        4 => {
            list.push(len_four(&s[0..4], false));
            list.push(d("10000").into());
        }
        5 => {
            list.push(d(&s[0..1]).into());
            list.push(d("100000000").into());
            list.push(len_four(&s[1..5], false));
            if &s[1..5] != "0000" {
                list.push(d("10000").into());
            }
        }
        _ => unreachable!("9桁まで"),
    }
    list.push(len_four(&s[s.len() - 4..], false));
    list.join(" ")
}

/// 数字だけの文字列(ASCII)をひらがなにする。空白は取り除く。
pub fn convert(digits: &str) -> String {
    let digits = digits.replace(',', "");
    if digits.len() > 9 {
        // 元の実装は英語のエラー文を返す。ここでは1桁ずつ読む。
        return digits.chars().map(|c| d(&c.to_string())).collect();
    }
    let mut n = digits.as_str();
    while n.starts_with('0') && n.len() > 1 {
        n = &n[1..];
    }
    let r = match n.len() {
        1 => len_one(n),
        2 => len_two(n),
        3 => len_three(n),
        4 => len_four(n, true),
        _ => len_x(n),
    };
    r.chars().filter(|c| *c != ' ').collect()
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn matches_known_readings() {
        assert_eq!(convert("0"), "ゼロ");
        assert_eq!(convert("10"), "じゅう");
        assert_eq!(convert("17"), "じゅうなな");
        assert_eq!(convert("300"), "さんびゃく");
        assert_eq!(convert("1000"), "せん");
        assert_eq!(convert("2026"), "にせんにじゅうろく");
        assert_eq!(convert("10000"), "いちまん");
        assert_eq!(convert("120000000"), "いちおくにせんまん");
        assert_eq!(convert("1,234"), "せんにひゃくさんじゅうよん");
        assert_eq!(convert("007"), "なな");
    }
}
