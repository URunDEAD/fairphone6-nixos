//! fp6-kbd-ngram: small word n-gram models for maliit-keyboard word prediction.
//!
//! The model is a plain UTF-8 TSV file read by the patched maliit-keyboard
//! (plugins/westernsupport/ngrampredictor.cpp). The scoring here mirrors the
//! C++ predictor so `predict` and `eval` show what the keyboard will offer.
//!
//! File format (version 1), one record per line, tab separated:
//!   #fp6-ngram  1            header (first line)
//!   #lang       ro           metadata lines start with '#'
//!   1  word  Surface  count  unigram: lowercase key, display form, count
//!   2  w1  w2  count         bigram (lowercase keys; w1 may be "<s>")
//!   3  w1  w2  w3  count     trigram
//! "<s>" marks the start of a sentence. Records are sorted, so the output is
//! reproducible byte for byte.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

pub const FORMAT_VERSION: u32 = 1;
pub const SENTENCE_START: &str = "<s>";
/// Backoff weights (stupid backoff). Keep in sync with ngrampredictor.cpp.
pub const BACKOFF_BIGRAM: f64 = 0.4;
pub const BACKOFF_UNIGRAM: f64 = 0.16;
/// Weight of learned (user) counts relative to the shipped model.
pub const USER_WEIGHT: f64 = 4.0;

/// Language-specific text normalisation applied before tokenising.
///
/// Romanian: the cedilla letters ş ţ Ş Ţ (U+015F/U+0163/U+015E/U+0162) and
/// decomposed s/t + U+0327 or U+0326 are mapped to the comma-below letters
/// ș ț Ș Ț (U+0219/U+021B/U+0218/U+021A). ã/Ã (a legacy 8-bit encoding
/// artefact for ă) becomes ă/Ă, grave-accented vowels (old stress marks)
/// lose the accent and the old in-word apostrophe becomes a hyphen.
pub fn normalize(lang: &str, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let ro = lang == "ro";
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let c = match c {
            '\u{2019}' | '\u{2018}' | '\u{02BC}' => '\'',
            '\u{2010}' | '\u{2011}' => '-',
            _ => c,
        };
        if ro {
            // Combining comma below / cedilla after s, t.
            if matches!(c, 's' | 't' | 'S' | 'T')
                && matches!(chars.peek(), Some('\u{0326}') | Some('\u{0327}'))
            {
                chars.next();
                out.push(match c {
                    's' => 'ș',
                    't' => 'ț',
                    'S' => 'Ș',
                    _ => 'Ț',
                });
                continue;
            }
            out.push(match c {
                // 19th-century stress marks and the old apostrophe for the
                // modern hyphen ("s'a" -> "s-a").
                'à' => 'a',
                'è' => 'e',
                'ì' => 'i',
                'ò' => 'o',
                'ù' => 'u',
                'À' => 'A',
                'È' => 'E',
                'Ì' => 'I',
                'Ò' => 'O',
                'Ù' => 'U',
                '\'' => '-',
                'ş' => 'ș',
                'ţ' => 'ț',
                'Ş' => 'Ș',
                'Ţ' => 'Ț',
                'ã' => 'ă',
                'Ã' => 'Ă',
                other => other,
            });
        } else {
            out.push(c);
        }
    }
    if lang == "en" {
        // Victorian spellings in the Dorian Gray text.
        for (old, new) in [("to-morrow", "tomorrow"), ("To-morrow", "Tomorrow"), ("to-night", "tonight"),
                           ("To-night", "Tonight"), ("to-day", "today"), ("To-day", "Today")] {
            out = out.replace(old, new);
        }
    }
    out
}

/// Accent folding used for diacritic-insensitive prefix matching (typing
/// "fata" offers "față"/"fată"). Keep the table in sync with the C++ one.
pub fn fold_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ł' | 'ľ' | 'ĺ' | 'ļ' => 'l',
        'ñ' | 'ń' | 'ň' | 'ņ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => 's',
        'ţ' | 'ť' | 'ț' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

pub fn lower(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

fn is_word_char(c: char) -> bool {
    c.is_alphabetic()
}

fn is_joiner(c: char) -> bool {
    c == '-' || c == '\''
}

/// Characters that end a sentence (and reset the prediction context).
/// Matches the keyboard: maliit-keyboard's sentence-break set "!.?" plus
/// the separators that start a new clause in practice.
pub fn is_sentence_break(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…' | ';' | ':' | '\n' | '\r')
}

/// Split text into sentences of words. Words are runs of letters with
/// internal '-' or '\'' ("într-o", "s-au", "don't"). Tokens containing
/// digits or other symbols are dropped but do not break the sentence.
/// In corpus mode single newlines are soft wraps; blank lines break.
pub fn sentences(text: &str, corpus_mode: bool) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut junk = false; // current token has a digit/symbol inside
    let chars: Vec<char> = text.chars().collect();
    let flush_word = |word: &mut String, junk: &mut bool, cur: &mut Vec<String>| {
        let w = word.trim_matches(|c| is_joiner(c)).to_string();
        if !*junk && !w.is_empty() && w.chars().any(is_word_char) {
            cur.push(w);
        }
        word.clear();
        *junk = false;
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_word_char(c) {
            word.push(c);
        } else if is_joiner(c)
            && !word.is_empty()
            && i + 1 < chars.len()
            && is_word_char(chars[i + 1])
        {
            word.push(c);
        } else if c.is_ascii_digit() && !word.is_empty() {
            junk = true;
            word.push(c);
        } else {
            flush_word(&mut word, &mut junk, &mut cur);
            let brk = if corpus_mode && c == '\n' {
                // blank line = paragraph break; single newline = soft wrap
                let mut j = i + 1;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\r') {
                    j += 1;
                }
                j < chars.len() && chars[j] == '\n'
            } else {
                is_sentence_break(c)
            };
            if brk && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        i += 1;
    }
    flush_word(&mut word, &mut junk, &mut cur);
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Default, Debug, Clone)]
pub struct Model {
    pub lang: String,
    pub meta: Vec<(String, String)>,
    /// lowercase -> (surface, count)
    pub uni: HashMap<String, (String, u64)>,
    /// w1 -> w2 -> count
    pub bi: HashMap<String, HashMap<String, u64>>,
    /// (w1, w2) -> w3 -> count
    pub tri: HashMap<(String, String), HashMap<String, u64>>,
    pub total: u64,
    bi_ctx: HashMap<String, u64>,
    tri_ctx: HashMap<(String, String), u64>,
    /// Unigram keys sorted, and (folded, key) pairs sorted, for prefix ranges.
    by_exact: Vec<String>,
    by_folded: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    pub min_unigram: u64,
    pub min_bigram: u64,
    pub min_trigram: u64,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions { min_unigram: 1, min_bigram: 1, min_trigram: 2 }
    }
}

/// Accumulates counts over several corpora (each with a weight).
#[derive(Default)]
pub struct Builder {
    lang: String,
    // lowercase -> surface form -> count (only non-sentence-initial forms)
    surfaces: HashMap<String, HashMap<String, u64>>,
    uni: HashMap<String, u64>,
    bi: HashMap<(String, String), u64>,
    tri: HashMap<(String, String, String), u64>,
    tokens: u64,
}

impl Builder {
    pub fn new(lang: &str) -> Self {
        Builder { lang: lang.to_string(), ..Default::default() }
    }

    pub fn add_text(&mut self, text: &str, weight: u64) {
        let text = normalize(&self.lang, text);
        for sent in sentences(&text, true) {
            let lw: Vec<String> = sent.iter().map(|w| lower(w)).collect();
            for (i, w) in sent.iter().enumerate() {
                let l = &lw[i];
                self.tokens += weight;
                *self.uni.entry(l.clone()).or_default() += weight;
                if i > 0 {
                    *self
                        .surfaces
                        .entry(l.clone())
                        .or_default()
                        .entry(w.clone())
                        .or_default() += weight;
                }
                let h1 = if i >= 1 { lw[i - 1].clone() } else { SENTENCE_START.to_string() };
                *self.bi.entry((h1.clone(), l.clone())).or_default() += weight;
                let h2 = if i >= 2 {
                    lw[i - 2].clone()
                } else if i == 1 {
                    SENTENCE_START.to_string()
                } else {
                    continue;
                };
                *self.tri.entry((h2, h1, l.clone())).or_default() += weight;
            }
        }
    }

    pub fn finish(self, opts: BuildOptions, meta: Vec<(String, String)>) -> Model {
        let mut m = Model { lang: self.lang.clone(), meta, ..Default::default() };
        for (l, c) in &self.uni {
            if *c < opts.min_unigram {
                continue;
            }
            // Display form: most common mid-sentence spelling if it beats the
            // lowercase one (proper nouns, "I"); otherwise lowercase.
            let surface = self
                .surfaces
                .get(l)
                .and_then(|s| {
                    let mut v: Vec<_> = s.iter().collect();
                    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                    v.first().map(|(w, n)| ((*w).clone(), **n))
                })
                .filter(|(w, n)| w != l && *n * 2 > self.surfaces[l].values().sum::<u64>())
                .map(|(w, _)| w)
                .unwrap_or_else(|| l.clone());
            m.uni.insert(l.clone(), (surface, *c));
        }
        let known = |w: &str| w == SENTENCE_START || m.uni.contains_key(w);
        let mut bi: HashMap<String, HashMap<String, u64>> = HashMap::new();
        for ((a, b), c) in &self.bi {
            if *c >= opts.min_bigram && known(a) && known(b) {
                bi.entry(a.clone()).or_default().insert(b.clone(), *c);
            }
        }
        let mut tri: HashMap<(String, String), HashMap<String, u64>> = HashMap::new();
        for ((a, b, w), c) in &self.tri {
            if *c >= opts.min_trigram && known(a) && known(b) && known(w) {
                tri.entry((a.clone(), b.clone())).or_default().insert(w.clone(), *c);
            }
        }
        m.bi = bi;
        m.tri = tri;
        m.reindex();
        m
    }
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    BadHeader,
    BadLine(usize),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::BadHeader => write!(f, "not an fp6-ngram v{FORMAT_VERSION} file"),
            ParseError::BadLine(n) => write!(f, "malformed record on line {n}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl Model {
    pub fn empty(lang: &str) -> Self {
        Model { lang: lang.to_string(), ..Default::default() }
    }

    fn reindex(&mut self) {
        self.total = self.uni.values().map(|(_, c)| c).sum();
        self.bi_ctx = self.bi.iter().map(|(k, v)| (k.clone(), v.values().sum())).collect();
        self.tri_ctx = self.tri.iter().map(|(k, v)| (k.clone(), v.values().sum())).collect();
        self.by_exact = self.uni.keys().cloned().collect();
        self.by_exact.sort();
        self.by_folded = self.uni.keys().map(|k| (fold(k), k.clone())).collect();
        self.by_folded.sort();
    }

    /// Unigram keys starting with `prefix` (folded comparison if `folded`).
    pub fn with_prefix<'a>(&'a self, prefix: &'a str, folded: bool) -> Box<dyn Iterator<Item = &'a String> + 'a> {
        if folded {
            let start = self.by_folded.partition_point(|(f, _)| f.as_str() < prefix);
            Box::new(self.by_folded[start..].iter().take_while(move |(f, _)| f.starts_with(prefix)).map(|(_, k)| k))
        } else {
            let start = self.by_exact.partition_point(|k| k.as_str() < prefix);
            Box::new(self.by_exact[start..].iter().take_while(move |k| k.starts_with(prefix)))
        }
    }

    /// Serialise deterministically (sorted records).
    pub fn to_tsv(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "#fp6-ngram\t{FORMAT_VERSION}");
        let _ = writeln!(s, "#lang\t{}", self.lang);
        for (k, v) in &self.meta {
            let _ = writeln!(s, "#{}\t{}", k, v.replace(['\t', '\n'], " "));
        }
        let uni: BTreeMap<_, _> = self.uni.iter().collect();
        for (l, (surface, c)) in uni {
            let _ = writeln!(s, "1\t{l}\t{surface}\t{c}");
        }
        let mut bi: Vec<_> = self
            .bi
            .iter()
            .flat_map(|(a, m)| m.iter().map(move |(b, c)| (a, b, c)))
            .collect();
        bi.sort();
        for (a, b, c) in bi {
            let _ = writeln!(s, "2\t{a}\t{b}\t{c}");
        }
        let mut tri: Vec<_> = self
            .tri
            .iter()
            .flat_map(|((a, b), m)| m.iter().map(move |(w, c)| (a, b, w, c)))
            .collect();
        tri.sort();
        for (a, b, w, c) in tri {
            let _ = writeln!(s, "3\t{a}\t{b}\t{w}\t{c}");
        }
        s
    }

    pub fn from_tsv(text: &str) -> Result<Model, ParseError> {
        let mut m = Model::default();
        let mut lines = text.lines().enumerate();
        match lines.next() {
            Some((_, l)) if l == format!("#fp6-ngram\t{FORMAT_VERSION}") => {}
            _ => return Err(ParseError::BadHeader),
        }
        for (n, line) in lines {
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix('#') {
                let (k, v) = rest.split_once('\t').unwrap_or((rest, ""));
                if k == "lang" {
                    m.lang = v.to_string();
                } else {
                    m.meta.push((k.to_string(), v.to_string()));
                }
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            let bad = || ParseError::BadLine(n + 1);
            let count = |s: &str| s.parse::<u64>().map_err(|_| bad());
            match (f.first().copied(), f.len()) {
                (Some("1"), 4) => {
                    m.uni.insert(f[1].to_string(), (f[2].to_string(), count(f[3])?));
                }
                (Some("2"), 4) => {
                    m.bi.entry(f[1].to_string()).or_default().insert(f[2].to_string(), count(f[3])?);
                }
                (Some("3"), 5) => {
                    m.tri
                        .entry((f[1].to_string(), f[2].to_string()))
                        .or_default()
                        .insert(f[3].to_string(), count(f[4])?);
                }
                _ => return Err(bad()),
            }
        }
        m.reindex();
        Ok(m)
    }

    /// Learn one word in context (the keyboard's implicit learning).
    pub fn learn(&mut self, context: &[String], word: &str) {
        let l = lower(word);
        let h1 = context.last().cloned().unwrap_or_else(|| SENTENCE_START.to_string());
        // Keep the typed capitalisation of words seen mid-sentence (names).
        let surface = if h1 != SENTENCE_START { word.to_string() } else { l.clone() };
        let e = self.uni.entry(l.clone()).or_insert_with(|| (surface, 0));
        e.1 += 1;
        *self.bi.entry(h1.clone()).or_default().entry(l.clone()).or_default() += 1;
        if h1 != SENTENCE_START {
            let h2 = if context.len() >= 2 {
                context[context.len() - 2].clone()
            } else {
                SENTENCE_START.to_string()
            };
            *self.tri.entry((h2, h1)).or_default().entry(l).or_default() += 1;
        }
        self.reindex();
    }

    pub fn vocabulary(&self) -> usize {
        self.uni.len()
    }
}

/// Context = last two lowercase words of the text left of the cursor in the
/// current sentence; "<s>" stands in when the sentence has fewer words.
pub fn context_of(lang: &str, left: &str) -> Vec<String> {
    let norm = normalize(lang, left);
    // Text after the last sentence break.
    let tail = match norm.rfind(is_sentence_break) {
        Some(i) => &norm[i + norm[i..].chars().next().map_or(1, char::len_utf8)..],
        None => &norm[..],
    };
    let words: Vec<String> = sentences(tail, false)
        .into_iter()
        .flatten()
        .map(|w| lower(&w))
        .collect();
    let mut ctx = vec![SENTENCE_START.to_string(), SENTENCE_START.to_string()];
    ctx.extend(words);
    ctx[ctx.len() - 2..].to_vec()
}

/// Rank completions of `prefix` given `context` (from `context_of`).
/// Returns display forms, best first, never the prefix itself.
pub fn predict(base: &Model, user: Option<&Model>, context: &[String], prefix: &str, limit: usize) -> Vec<String> {
    let pl = lower(prefix);
    if pl.is_empty() {
        return Vec::new();
    }
    let folded_mode = fold(&pl) == pl;
    let matches = |w: &str| {
        if folded_mode {
            fold(w).starts_with(&pl)
        } else {
            w.starts_with(&pl)
        }
    };
    let empty = Model::default();
    let user = user.unwrap_or(&empty);
    let uw = USER_WEIGHT;
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut bump = |w: &str, s: f64| {
        let e = scores.entry(w.to_string()).or_insert(0.0);
        if s > *e {
            *e = s;
        }
    };
    let h1 = context.last().cloned().unwrap_or_else(|| SENTENCE_START.to_string());
    let h2 = if context.len() >= 2 { context[context.len() - 2].clone() } else { SENTENCE_START.to_string() };
    // trigram
    let key = (h2, h1.clone());
    let d3 = *base.tri_ctx.get(&key).unwrap_or(&0) as f64 + uw * *user.tri_ctx.get(&key).unwrap_or(&0) as f64;
    if d3 > 0.0 {
        for m in [base.tri.get(&key), user.tri.get(&key)].into_iter().flatten() {
            for w in m.keys() {
                if matches(w) {
                    let c = count3(base, &key, w) + uw * count3(user, &key, w);
                    bump(w, c / d3);
                }
            }
        }
    }
    // bigram
    let d2 = *base.bi_ctx.get(&h1).unwrap_or(&0) as f64 + uw * *user.bi_ctx.get(&h1).unwrap_or(&0) as f64;
    if d2 > 0.0 {
        for m in [base.bi.get(&h1), user.bi.get(&h1)].into_iter().flatten() {
            for w in m.keys() {
                if matches(w) {
                    let c = count2(base, &h1, w) + uw * count2(user, &h1, w);
                    bump(w, BACKOFF_BIGRAM * c / d2);
                }
            }
        }
    }
    // unigram
    let d1 = base.total as f64 + uw * user.total as f64;
    if d1 > 0.0 {
        for m in [base, user] {
            for w in m.with_prefix(&pl, folded_mode) {
                {
                    let c = base.uni.get(w).map_or(0, |e| e.1) as f64 + uw * user.uni.get(w).map_or(0, |e| e.1) as f64;
                    bump(w, BACKOFF_UNIGRAM * c / d1);
                }
            }
        }
    }
    let mut v: Vec<(String, f64)> = scores.into_iter().filter(|(w, _)| *w != pl).collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    v.into_iter()
        .take(limit)
        .map(|(w, _)| {
            base.uni
                .get(&w)
                .or_else(|| user.uni.get(&w))
                .map(|e| e.0.clone())
                .unwrap_or(w)
        })
        .collect()
}

fn count3(m: &Model, key: &(String, String), w: &str) -> f64 {
    m.tri.get(key).and_then(|x| x.get(w)).copied().unwrap_or(0) as f64
}

fn count2(m: &Model, h: &str, w: &str) -> f64 {
    m.bi.get(h).and_then(|x| x.get(w)).copied().unwrap_or(0) as f64
}

/// Keystroke-savings evaluation: type each word of `text` letter by letter,
/// accept the word as soon as it is among the top `k` candidates (one tap).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct EvalResult {
    pub words: u64,
    pub letters: u64,
    pub keystrokes: u64,
    pub top_k_after_1: u64,
    pub top_k_after_2: u64,
}

impl EvalResult {
    pub fn savings(&self) -> f64 {
        if self.letters == 0 {
            0.0
        } else {
            1.0 - self.keystrokes as f64 / self.letters as f64
        }
    }
}

pub fn evaluate(model: &Model, lang: &str, text: &str, k: usize) -> EvalResult {
    let mut r = EvalResult::default();
    let text = normalize(lang, text);
    for sent in sentences(&text, true) {
        let mut ctx = vec![SENTENCE_START.to_string(), SENTENCE_START.to_string()];
        for w in &sent {
            let l = lower(w);
            let n = l.chars().count() as u64;
            r.words += 1;
            r.letters += n;
            let mut cost = n;
            let chars: Vec<char> = l.chars().collect();
            for typed in 1..chars.len() {
                let prefix: String = chars[..typed].iter().collect();
                let preds: Vec<String> = predict(model, None, &ctx[ctx.len() - 2..], &prefix, k)
                    .into_iter()
                    .map(|p| lower(&p))
                    .collect();
                let hit = preds.iter().any(|p| *p == l);
                if typed == 1 && hit {
                    r.top_k_after_1 += 1;
                }
                if typed <= 2 && hit {
                    r.top_k_after_2 += 1;
                }
                if hit {
                    cost = typed as u64 + 1;
                    break;
                }
            }
            r.keystrokes += cost.min(n);
            ctx.push(l);
        }
    }
    r
}

/// (left context, prefix) pairs for every word of `text`: the sentence so
/// far plus the first one and two letters of the word.
pub fn parity_queries(lang: &str, text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for sent in sentences(&normalize(lang, text), true) {
        for (i, w) in sent.iter().enumerate() {
            let left = if i == 0 { String::new() } else { format!("{} ", sent[..i].join(" ")) };
            let chars: Vec<char> = w.chars().collect();
            for n in 1..=2.min(chars.len()) {
                out.push((left.clone(), chars[..n].iter().collect()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn romanian_cedilla_becomes_comma_below() {
        assert_eq!(normalize("ro", "şi ţară ŞI ŢARĂ"), "și țară ȘI ȚARĂ");
        assert_eq!(normalize("ro", "s\u{0327}i t\u{0326}ara"), "și țara");
        assert_eq!(normalize("ro", "mãr"), "măr");
        assert_eq!(normalize("ro", "făcù s\u{2019}a și'n"), "făcu s-a și-n");
        // other languages untouched
        assert_eq!(normalize("tr", "şeker"), "şeker");
        assert_eq!(normalize("en", "don\u{2019}t"), "don't");
        assert_eq!(normalize("en", "See you to-morrow, To-night"), "See you tomorrow, Tonight");
    }

    #[test]
    fn tokenizer_keeps_hyphenated_romanian_words() {
        let s = sentences("Ș-apoi într-o zi, s-au dus. Ce-i? 12 mere", true);
        assert_eq!(s, vec![vec!["Ș-apoi", "într-o", "zi", "s-au", "dus"], vec!["Ce-i"], vec!["mere"]]);
    }

    #[test]
    fn corpus_soft_wraps_and_paragraphs() {
        let s = sentences("one two\nthree\n\nfour", true);
        assert_eq!(s, vec![vec!["one", "two", "three"], vec!["four"]]);
        let s = sentences("one two\nthree", false);
        assert_eq!(s, vec![vec!["one", "two"], vec!["three"]]);
    }

    #[test]
    fn digits_drop_token_only() {
        let s = sentences("abc a1b def -x- 'y'", true);
        assert_eq!(s, vec![vec!["abc", "def", "x", "y"]]);
    }

    fn model(text: &str) -> Model {
        let mut b = Builder::new("ro");
        b.add_text(text, 1);
        b.finish(BuildOptions { min_unigram: 1, min_bigram: 1, min_trigram: 1 }, vec![])
    }

    #[test]
    fn counts_and_surface_forms() {
        let m = model("Ion merge acasă. Apoi Ion merge la Ion. ion");
        assert_eq!(m.uni["ion"], ("Ion".to_string(), 4));
        assert_eq!(m.uni["merge"].1, 2);
        assert_eq!(m.bi["ion"]["merge"], 2);
        assert_eq!(m.bi[SENTENCE_START]["ion"], 2);
        assert_eq!(m.tri[&("<s>".to_string(), "ion".to_string())]["merge"], 1);
        assert_eq!(m.total, 9);
    }

    #[test]
    fn tsv_round_trip_is_stable() {
        let m = model("și ea a zis. și el a zis că da");
        let t = m.to_tsv();
        let m2 = Model::from_tsv(&t).unwrap();
        assert_eq!(m2.to_tsv(), t);
        assert_eq!(m2.lang, "ro");
        assert!(t.starts_with("#fp6-ngram\t1\n#lang\tro\n"));
    }

    #[test]
    fn rejects_bad_files() {
        assert_eq!(Model::from_tsv("hello").unwrap_err(), ParseError::BadHeader);
        assert_eq!(Model::from_tsv("#fp6-ngram\t1\n1\tx\n").unwrap_err(), ParseError::BadLine(2));
        assert_eq!(Model::from_tsv("#fp6-ngram\t1\n2\ta\tb\tq\n").unwrap_err(), ParseError::BadLine(2));
    }

    #[test]
    fn context_extraction() {
        assert_eq!(context_of("ro", ""), vec!["<s>", "<s>"]);
        assert_eq!(context_of("ro", "Bună ziua. Ce "), vec!["<s>", "ce"]);
        assert_eq!(context_of("ro", "Mă duc acasă, şi "), vec!["acasă", "și"]);
        assert_eq!(context_of("en", "Hello!\n"), vec!["<s>", "<s>"]);
    }

    #[test]
    fn prediction_uses_context_and_backs_off() {
        let m = model("mă duc acasă. mă duc la școală. el doarme acum. ana are mere");
        let ctx = context_of("ro", "Mâine mă duc ");
        let p = predict(&m, None, &ctx, "a", 3);
        assert_eq!(p[0], "acasă");
        // unknown context: unigram backoff still completes
        let p = predict(&m, None, &context_of("ro", "zzz "), "doa", 3);
        assert_eq!(p, vec!["doarme"]);
        // the prefix itself is never returned
        let p = predict(&m, None, &ctx, "acasă", 3);
        assert!(p.is_empty());
    }

    #[test]
    fn diacritic_insensitive_unless_typed() {
        let m = model("față față fată fata ștergi");
        let ctx = context_of("ro", "");
        let p = predict(&m, None, &ctx, "fat", 5);
        assert_eq!(p, vec!["față", "fata", "fată"]);
        // typed diacritic: exact prefix only
        let p = predict(&m, None, &ctx, "fă", 5);
        assert!(p.is_empty());
        let p = predict(&m, None, &ctx, "fe", 5);
        assert!(p.is_empty());
        assert_eq!(predict(&m, None, &ctx, "ster", 5), vec!["ștergi"]);
        assert_eq!(predict(&m, None, &ctx, "ș", 5), vec!["ștergi"]);
    }

    #[test]
    fn user_learning_outranks_base() {
        let base = model("casa casa casa caiet");
        let mut user = Model::empty("ro");
        let ctx = context_of("ro", "");
        assert_eq!(predict(&base, Some(&user), &ctx, "ca", 1), vec!["casa"]);
        user.learn(&ctx, "caiet");
        user.learn(&ctx, "Caiet");
        assert_eq!(predict(&base, Some(&user), &ctx, "ca", 1), vec!["caiet"]);
        // words only the user knows are offered too
        user.learn(&ctx, "calculator");
        assert!(predict(&base, Some(&user), &ctx, "calc", 3).contains(&"calculator".to_string()));
    }

    #[test]
    fn eval_counts_keystrokes() {
        let m = model("acasă acasă acasă");
        let r = evaluate(&m, "ro", "acasă", 1);
        assert_eq!(r.words, 1);
        assert_eq!(r.letters, 5);
        assert_eq!(r.keystrokes, 2); // "a" + tap
        assert!((r.savings() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn parity_queries_cover_each_word() {
        let q = parity_queries("ro", "Ce faci? Eu");
        assert_eq!(
            q,
            vec![
                ("".to_string(), "C".to_string()),
                ("".to_string(), "Ce".to_string()),
                ("Ce ".to_string(), "f".to_string()),
                ("Ce ".to_string(), "fa".to_string()),
                ("".to_string(), "E".to_string()),
                ("".to_string(), "Eu".to_string()),
            ]
        );
    }

    #[test]
    fn deterministic_output_independent_of_order() {
        let mut a = Builder::new("en");
        a.add_text("the cat sat. a dog ran", 1);
        a.add_text("the cat ran", 2);
        let mut b = Builder::new("en");
        b.add_text("the cat ran", 2);
        b.add_text("the cat sat. a dog ran", 1);
        let o = BuildOptions::default();
        assert_eq!(a.finish(o, vec![]).to_tsv(), b.finish(o, vec![]).to_tsv());
    }
}
