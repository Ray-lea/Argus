//! The search box's history: the searches typed on a page, one list per page.
//!
//! A page keeps its own list (`searchHistory.<page>`), so the process page and
//! the service page never mix: switching pages switches the list too. The
//! newest entry comes first; repeats (ignoring case) move to the top instead
//! of piling up, and the list keeps the last [`MAX`] entries.
use super::prefs::Prefs;

/// How many searches a page remembers.
const MAX: usize = 10;

/// The preference key a page's history lives under.
fn key(page: &str) -> String {
    format!("searchHistory.{page}")
}

/// A page's searches, newest first.
pub(in crate::frontend) fn load(prefs: &Prefs, page: &str) -> Vec<String> {
    prefs
        .get::<Vec<String>>(&key(page), vec![])
        .into_iter()
        .map(|entry| entry.trim().to_owned())
        .filter(|entry| !entry.is_empty())
        .take(MAX)
        .collect()
}

/// Records a search on a page: blank text records nothing, and a search
/// already remembered moves to the top rather than appearing twice.
pub(in crate::frontend) fn record(prefs: &mut Prefs, page: &str, query: &str) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }
    let mut list = load(prefs, page);
    let position = list
        .iter()
        .position(|entry| entry.eq_ignore_ascii_case(query));
    match position {
        // Already first: nothing to write.
        Some(0) => return,
        Some(index) => {
            list.remove(index);
        }
        None => {}
    }
    list.insert(0, query.to_owned());
    list.truncate(MAX);
    prefs.set(&key(page), &list);
}

/// Forgets a page's searches.
pub(in crate::frontend) fn clear(prefs: &mut Prefs, page: &str) {
    prefs.set(&key(page), Vec::<String>::new());
}

#[cfg(test)]
mod tests {
    // Not the glob: the frontend's imports bring GPUI's own `test` attribute.
    use super::{clear, load, record};
    use crate::frontend::prefs::Prefs;

    fn store() -> Prefs {
        Prefs::load(std::env::temp_dir().join(format!(
            "argus-search-history-test-{}.json",
            std::process::id()
        )))
    }

    #[test]
    fn the_newest_search_leads_and_repeats_move_up() {
        let mut prefs = store();
        record(&mut prefs, "processes", "chrome");
        record(&mut prefs, "processes", " notepad ");
        record(&mut prefs, "processes", "CHROME");
        assert_eq!(load(&prefs, "processes"), ["CHROME", "notepad"]);
        // Blank text records nothing.
        record(&mut prefs, "processes", "   ");
        assert_eq!(load(&prefs, "processes"), ["CHROME", "notepad"]);
    }

    #[test]
    fn pages_keep_separate_lists_and_clear_only_their_own() {
        let mut prefs = store();
        record(&mut prefs, "processes", "360se");
        record(&mut prefs, "services", "spooler");
        assert_eq!(load(&prefs, "processes"), ["360se"]);
        assert_eq!(load(&prefs, "services"), ["spooler"]);
        clear(&mut prefs, "processes");
        assert!(load(&prefs, "processes").is_empty());
        assert_eq!(load(&prefs, "services"), ["spooler"]);
    }

    #[test]
    fn the_list_keeps_the_last_entries() {
        let mut prefs = store();
        for index in 0..15 {
            record(&mut prefs, "network", &format!("p{index}"));
        }
        let list = load(&prefs, "network");
        assert_eq!(list.len(), super::MAX);
        assert_eq!(list.first().map(String::as_str), Some("p14"));
        assert_eq!(list.last().map(String::as_str), Some("p5"));
    }
}
