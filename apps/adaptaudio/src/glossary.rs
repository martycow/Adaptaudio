//! The glossary, read from `docs/GLOSSARY.md` at build time so the docs stay the one source.

use std::sync::OnceLock;

const SOURCE: &str = include_str!("../../../docs/GLOSSARY.md");

#[derive(Clone, Debug)]
pub struct Entry {
    pub term: String,
    /// Lower-case names this entry answers to ("bpm", "beat", "доля").
    pub keys: Vec<String>,
    pub text: String,
    pub section: String,
}

fn parse() -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut section = String::new();
    let mut last: Option<usize> = None;
    for line in SOURCE.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            section = h.trim().to_string();
            last = None;
            continue;
        }
        let trimmed = line.trim_start();
        let nested = line.starts_with("  ");
        let Some(rest) = trimmed.strip_prefix("- **") else {
            continue;
        };
        let Some((head, tail)) = rest.split_once("**") else {
            continue;
        };
        let text = tail
            .trim_start_matches([' ', '—', '-', ':'])
            .trim()
            .replace("**", "")
            .replace(['*', '`'], "");
        let mut keys = Vec::new();
        // "Bar / Measure (такт)" → bar, measure, такт
        let (names, extra) = match head.split_once('(') {
            Some((n, e)) => (n, e.trim_end_matches(')')),
            None => (head, ""),
        };
        for k in names.split('/').chain(extra.split(['/', ','])) {
            let k = k.trim().to_lowercase();
            if !k.is_empty() {
                keys.push(k);
            }
        }
        let entry = Entry {
            term: head.trim().to_string(),
            keys,
            text,
            section: section.clone(),
        };
        if nested && let Some(i) = last {
            // Sub-items (ADSR stages, waveforms) become entries of their own too.
            out.push(Entry {
                section: out[i].section.clone(),
                ..entry
            });
            continue;
        }
        out.push(entry);
        last = Some(out.len() - 1);
    }
    out
}

pub fn entries() -> &'static [Entry] {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(parse)
}

/// The explanation for a word shown in the UI, if the glossary has it.
pub fn lookup(word: &str) -> Option<String> {
    let w = word.trim().to_lowercase();
    entries()
        .iter()
        .find(|e| e.keys.contains(&w))
        .map(|e| format!("{} — {}", e.term, e.text))
}

/// Entries whose term or text contains `query` (case-insensitive).
pub fn search(query: &str) -> Vec<&'static Entry> {
    let q = query.trim().to_lowercase();
    entries()
        .iter()
        .filter(|e| {
            q.is_empty() || e.term.to_lowercase().contains(&q) || e.text.to_lowercase().contains(&q)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glossary_parses_and_answers_ui_words() {
        assert!(entries().len() > 80, "{}", entries().len());
        for w in [
            "BPM",
            "Cutoff",
            "Sidechain",
            "Influence",
            "Attack",
            "Saw",
            "Intensity",
            "Stinger",
        ] {
            assert!(lookup(w).is_some(), "{w} missing");
        }
        assert!(
            search("сайдчейн")
                .iter()
                .any(|e| e.term.starts_with("Sidechain"))
        );
    }
}
