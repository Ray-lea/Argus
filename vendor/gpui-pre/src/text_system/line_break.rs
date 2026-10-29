//! Line break opportunities around non-ASCII text (Argus patch).
//!
//! The wrappers' own rules only know spaces and a few ASCII marks: they broke
//! before any other character, so a wrapped line could start with `。`, `，`
//! or `）` and end with `（` or `“`. Chromium, which the original front end
//! used, breaks such text by UAX #14 through ICU (`line-break: auto`). Pairs
//! with a non-ASCII character follow the same rules here; ASCII pairs keep
//! the wrappers' rules, as Blink also handles them apart from ICU.
//!
//! The decisions match what the original's WebView2 (Chromium 155, `lang`
//! zh-CN) does, measured pair by pair: UAX #14 of Unicode 16 with ICU's
//! normal CJK tailoring (small kana, `ー` and `〜` break like ideographs) and
//! `“` `”` as opening and closing punctuation, while `‘` `’` keep the East
//! Asian quotation rules. Rules that need more than one character of context
//! on either side (numbers with several separators, runs of regional
//! indicators) are left out.

use unicode_linebreak::{BreakClass, break_property};

use BreakClass::{
    After as BA, Alphabetic as AL, Ambiguous as AI, Before as BB, BeforeAndAfter as B2,
    CarriageReturn as CR, CloseParenthesis as CP, ClosePunctuation as CL, CombiningMark as CM,
    ComplexContext as SA, ConditionalJapaneseStarter as CJ, Contingent as CB, EmojiBase as EB,
    EmojiModifier as EM, Exclamation as EX, HangulLJamo as JL, HangulLvSyllable as H2,
    HangulLvtSyllable as H3, HangulTJamo as JT, HangulVJamo as JV, HebrewLetter as HL,
    Hyphen as HY, Ideographic as ID, InfixSeparator as IS, Inseparable as IN, LineFeed as LF,
    Mandatory as BK, NextLine as NL, NonBreakingGlue as GL, NonStarter as NS, Numeric as NU,
    OpenPunctuation as OP, Postfix as PO, Prefix as PR, Quotation as QU, RegionalIndicator as RI,
    Space as SP, Surrogate as SG, Symbol as SY, Unknown as XX, WordJoiner as WJ,
    ZeroWidthJoiner as ZWJ, ZeroWidthSpace as ZW,
};

/// The character's class after the resolution in UAX #14 LB1 and ICU's
/// tailoring for Chinese and Japanese text.
fn class(c: char) -> BreakClass {
    match c {
        '\u{201C}' => return OP,
        '\u{201D}' => return CL,
        // A wave dash or double hyphen allows breaks on both sides.
        '\u{301C}' | '\u{30A0}' => return ID,
        _ => {}
    }
    match break_property(c as u32) {
        AI | SG | XX | SA => AL,
        CJ => ID,
        class => class,
    }
}

/// East Asian Width F, W or H (`$EastAsian` in UAX #14), by block: the CJK,
/// Hangul, fullwidth and halfwidth forms and the wide emoji ranges.
fn east_asian(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{115F}'
            | '\u{231A}'..='\u{231B}'
            | '\u{2329}'..='\u{232A}'
            | '\u{2E80}'..='\u{303E}'
            | '\u{3041}'..='\u{33FF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{A000}'..='\u{A4CF}'
            | '\u{A960}'..='\u{A97F}'
            | '\u{AC00}'..='\u{D7A3}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE10}'..='\u{FE19}'
            | '\u{FE30}'..='\u{FE6F}'
            | '\u{FF00}'..='\u{FFEE}'
            | '\u{1F300}'..='\u{1F64F}'
            | '\u{1F900}'..='\u{1F9FF}'
            | '\u{20000}'..='\u{3FFFD}')
}

fn initial_quote(c: char) -> bool {
    matches!(
        c,
        '\u{00AB}'
            | '\u{2018}'
            | '\u{201B}'
            | '\u{201F}'
            | '\u{2039}'
            | '\u{2E02}'
            | '\u{2E04}'
            | '\u{2E09}'
            | '\u{2E0C}'
            | '\u{2E1C}'
            | '\u{2E20}'
    )
}

fn final_quote(c: char) -> bool {
    matches!(
        c,
        '\u{00BB}'
            | '\u{2019}'
            | '\u{203A}'
            | '\u{2E03}'
            | '\u{2E05}'
            | '\u{2E0A}'
            | '\u{2E0D}'
            | '\u{2E1D}'
            | '\u{2E21}'
    )
}

/// Whether a line may break between `before` and `after` by UAX #14, given
/// the characters around them (`'\0'` at the start or end of the text).
///
/// Spaces are the wrappers' business: a line may break after a space (Blink
/// allows that whatever follows) and never before one.
pub(crate) fn break_allowed(before2: char, before: char, after: char, after2: char) -> bool {
    // The common case in Chinese text: two ideographs (ID ÷ ID).
    let han = |c: char| matches!(c, '\u{4E00}'..='\u{9FFF}');
    if han(before) && han(after) {
        return true;
    }
    let a = class(after);
    if matches!(a, BK | CR | LF | NL | SP | ZW) {
        return false;
    }
    let mut b = class(before);
    if b == SP || b == ZW {
        return true;
    }
    if matches!(b, BK | CR | LF | NL) {
        return true;
    }
    // LB8a, LB9: joiners and combining marks stay with what they follow.
    if b == ZWJ || matches!(a, CM | ZWJ) {
        return false;
    }
    // LB10: a combining mark with nothing to attach to acts as a letter.
    if b == CM {
        b = AL;
    }
    // LB11, LB12, LB12a: glue.
    if a == WJ || b == WJ || b == GL || (a == GL && !matches!(b, BA | HY)) {
        return false;
    }
    // LB13, LB15d: closing punctuation and separators never start a line.
    if matches!(a, CL | CP | EX | SY | IS) {
        return false;
    }
    // LB14: opening punctuation never ends one.
    if b == OP {
        return false;
    }
    // LB17: dashes stay together.
    if b == B2 && a == B2 {
        return false;
    }
    // LB19, LB19a: quotation marks keep to the text they quote. Initial
    // quotes may open a line, and final quotes close one, among East Asian
    // characters only.
    if a == QU {
        if !initial_quote(after) {
            return false;
        }
        if !east_asian(before) || after2 == '\0' || !east_asian(after2) {
            return false;
        }
    }
    if b == QU {
        if !final_quote(before) {
            return false;
        }
        if !east_asian(after) || before2 == '\0' || !east_asian(before2) {
            return false;
        }
    }
    // LB20: contingent breaks.
    if a == CB || b == CB {
        return true;
    }
    // LB20a: a hyphen that starts a word stays with the letters after it.
    if (b == HY || matches!(before, '\u{2010}' | '\u{2013}'))
        && matches!(a, AL | HL)
        && (before2 == '\0' || matches!(class(before2), SP | BK | CR | LF | NL | ZW | CB | GL))
    {
        return false;
    }
    // LB21: hyphens, small marks and iteration marks never start a line.
    if matches!(a, BA | HY | NS) || b == BB {
        return false;
    }
    // LB21a: a Hebrew letter and a hyphen stay with what follows.
    if matches!(b, HY | BA) && a != HL && class(before2) == HL {
        return false;
    }
    // LB21b, LB22.
    if (b == SY && a == HL) || a == IN {
        return false;
    }
    // LB23, LB23a, LB24: letters, numbers and their prefixes and suffixes.
    if (matches!(b, AL | HL) && a == NU) || (b == NU && matches!(a, AL | HL)) {
        return false;
    }
    let dash = |c: char| matches!(c, '\u{301C}' | '\u{30A0}');
    if (b == PR && matches!(a, ID | EB | EM) && !dash(after))
        || (matches!(b, ID | EB | EM) && a == PO && !dash(before))
    {
        return false;
    }
    if (matches!(b, PR | PO) && matches!(a, AL | HL))
        || (matches!(b, AL | HL) && matches!(a, PR | PO))
    {
        return false;
    }
    // LB25: numbers, by pairs; a separator or closing mark keeps a suffix or
    // prefix only after a digit.
    if (matches!(b, PR | PO) && a == NU)
        || (b == NU && matches!(a, PO | PR | NU))
        || (matches!(b, CL | CP | IS | SY) && matches!(a, PO | PR) && class(before2) == NU)
        || (matches!(b, HY | IS | SY) && a == NU)
        || (matches!(b, PR | PO) && matches!(a, OP | HY) && class(after2) == NU)
    {
        return false;
    }
    // LB26, LB27: Hangul syllables.
    if (b == JL && matches!(a, JL | JV | H2 | H3))
        || (matches!(b, JV | H2) && matches!(a, JV | JT))
        || (matches!(b, JT | H3) && a == JT)
        || (matches!(b, JL | JV | JT | H2 | H3) && a == PO)
        || (b == PR && matches!(a, JL | JV | JT | H2 | H3))
    {
        return false;
    }
    // LB28, LB29: words.
    if matches!(a, AL | HL) && matches!(b, AL | HL | IS) {
        return false;
    }
    // LB30: letters and numbers stay with parentheses that are not East Asian.
    if (matches!(b, AL | HL | NU) && a == OP && !east_asian(after))
        || (b == CP && !east_asian(before) && matches!(a, AL | HL | NU))
    {
        return false;
    }
    // LB30a, LB30b: flags and emoji modifiers.
    if (b == RI && a == RI) || (b == EB && a == EM) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::break_allowed;

    /// The characters of `CHROMIUM`, in order.
    const CHARS: &str = "\u{4E2D}a1\u{3002}\u{FF0C}\u{3001}\u{FF1A}\u{FF01}\u{FF1F}\u{FF08}\u{FF09}\u{300C}\u{300D}\u{300A}\u{300B}\u{201C}\u{201D}\u{2026}\u{2014}\u{FF5E}\u{B7}\u{FF05}\u{FFE5}\u{3041}\u{30FC}\u{3005}\u{301C}\u{2013}\u{3000}\u{A0}\u{AC00}\u{E9}";
    /// Whether Chromium 155 (WebView2, `lang="zh-CN"`) breaks between each
    /// pair at the start and end of a text: row character, then column
    /// character; `.` marks ASCII pairs, which the wrappers decide.
    const CHROMIUM: [&str; 32] = [
        "11100000010101010011101110100011", // 中
        "1..00000010101000011000110100010", // a
        "1..00000010101000011000110100010", // 1
        "11100000010101010011111110100011", // 。
        "11100000010101010011111110100011", // ，
        "11100000010101010011111110100011", // 、
        "11100000010101010011111110100011", // ：
        "11100000010101010011111110100011", // ！
        "11100000010101010011111110100011", // ？
        "00000000000000000000000000000000", // （
        "11100000010101010011111110100011", // ）
        "00000000000000000000000000000000", // 「
        "11100000010101010011111110100011", // 」
        "00000000000000000000000000000000", // 《
        "11100000010101010011111110100011", // 》
        "00000000000000000000000000000000", // “
        "11100000010101010011111110100011", // ”
        "11100000010101010011111110100011", // …
        "11100000010101010001111110100011", // —
        "11100000010101010011101110100011", // ～
        "10000000010101000011000110100010", // ·
        "10000000010101010011011110100010", // ％
        "00000000010101010010011000100000", // ￥
        "11100000010101010011101110100011", // ぁ
        "11100000010101010011101110100011", // ー
        "11100000010101010011111110100011", // 々
        "11100000010101010011111110100011", // 〜
        "10100000010101010011011110100110", // –
        "11100000010101010011111110100111", //
        "00000000000000000000000000000000", //
        "11100000010101010011101110100011", // 가
        "10000000010101000011000110100010", // é
    ];

    #[test]
    fn pairs_break_as_in_chromium() {
        let chars: Vec<char> = CHARS.chars().collect();
        for (row, before) in CHROMIUM.iter().zip(&chars) {
            for (expected, after) in row.chars().zip(&chars) {
                if expected == '.' {
                    continue;
                }
                assert_eq!(
                    break_allowed('\0', *before, *after, '\0'),
                    expected == '1',
                    "{before:?} then {after:?}"
                );
            }
        }
    }

    /// Break positions (in characters) Chromium takes in each text when
    /// every opportunity is taken.
    fn breaks(text: &str) -> Vec<usize> {
        let chars: Vec<char> = text.chars().collect();
        (1..chars.len())
            .filter(|&i| {
                let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
                let before2 = if i >= 2 { at(i - 2) } else { '\0' };
                break_allowed(before2, at(i - 1), at(i), at(i + 1))
            })
            .collect()
    }

    #[test]
    fn context_breaks_as_in_chromium() {
        for (text, expected) in [
            // Quotation marks: `“` `”` open and close; `‘` `’` keep the East
            // Asian rules, so they need East Asian text on both sides.
            ("中“文”中", vec![1, 4]),
            ("中‘文’中", vec![1, 4]),
            ("中‘a", vec![]),
            ("a’中", vec![]),
            ("a“b”c", vec![4]),
            // Dashes and ellipses stay together.
            ("中——文", vec![1, 3]),
            ("中……文", vec![3]),
            ("中—a", vec![1, 2]),
            // Closing marks run on, opening marks stay with what follows.
            ("中、、文", vec![3]),
            ("中。」文", vec![3]),
            ("中「（文", vec![1]),
            // Ideographs break from letters and digits, not from affixes.
            ("第3个", vec![1, 2]),
            ("ab中cd", vec![2, 3]),
            ("中％", vec![]),
            ("中$", vec![1]),
            ("$中", vec![]),
            // A suffix stays with a number before a closing mark.
            ("1）％", vec![]),
            ("中）％", vec![2]),
            ("1.％", vec![]),
            // Hyphens and dashes: a break follows them inside a word.
            ("a–b", vec![2]),
            ("中‐b", vec![2]),
            ("é-é", vec![2]),
            // Wave dashes allow breaks on both sides.
            ("中〜1", vec![1, 2]),
            ("a゠b", vec![1, 2]),
        ] {
            assert_eq!(breaks(text), expected, "{text}");
        }
    }
}
