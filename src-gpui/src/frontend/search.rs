//! The list search box: one filter, or several separated by commas.
//!
//! A box holding `wxwork.exe,qq.exe` shows both processes: each term matches on
//! its own and a row stays when any of them matches. Terms are read (trimmed,
//! lowercased, a number when they are digits) once per list build.
/// One filter of the search box, lowercased for comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Term {
    /// The term as typed, lowercased: the values compared against it are
    /// lowercased too.
    pub text: String,
    /// A term of digits reads as a port or PID; services and connections then
    /// match it exactly.
    pub number: Option<u64>,
}

/// The terms a search box holds, in the order typed and without blank ones.
/// Commas separate terms, including the fullwidth comma of Chinese input;
/// spaces around a term are dropped, so `wxwork.exe, qq.exe` finds both.
pub(super) fn terms(raw: &str) -> Vec<Term> {
    raw.split([',', '，'])
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(|term| Term {
            text: term.to_lowercase(),
            number: term.parse().ok(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::terms;

    #[test]
    fn commas_separate_terms_and_blank_ones_are_dropped() {
        let text: Vec<String> = terms(" wxwork.exe , QQ.exe ,,")
            .into_iter()
            .map(|term| term.text)
            .collect();
        assert_eq!(text, ["wxwork.exe", "qq.exe"]);
        assert!(terms("").is_empty());
        assert!(terms("  ").is_empty());
        assert!(terms("，, ，").is_empty());
        assert_eq!(terms("wxwork.exe，qq.exe").len(), 2);
    }
    #[test]
    fn only_commas_separate_and_digit_terms_read_as_numbers() {
        let parsed = terms("8080,80 chrome");
        assert_eq!(parsed[0].number, Some(8080));
        // A space is part of a term: names and descriptions contain them.
        assert_eq!(parsed[1].text, "80 chrome");
        assert_eq!(parsed[1].number, None);
        assert_eq!(terms("0x10")[0].number, None);
    }
}
