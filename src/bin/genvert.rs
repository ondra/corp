use std::cmp::{max, min};
use std::io::{self, Write};

use pico_args::Arguments;

#[derive(Clone, Debug)]
struct Config {
    seed: u64,
    docs: usize,
    max_lines: Option<usize>,
    vocab: usize,
    zipf_s: f64,
    word_len: usize,
    lemma_group: usize,
    min_sent_len: usize,
    max_sent_len: usize,
    min_doc_words: usize,
    max_doc_words: usize,
    cap_prob: f64,
    pos_tags: Vec<char>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 0,
            docs: 1,
            max_lines: None,
            vocab: 20_000,
            zipf_s: 1.07,
            word_len: 6,
            lemma_group: 20,
            min_sent_len: 6,
            max_sent_len: 30,
            min_doc_words: 350,
            max_doc_words: 1500,
            cap_prob: 0.02,
            pos_tags: "nvadprstxi".chars().collect(),
        }
    }
}

fn print_usage(mut w: impl Write) -> io::Result<()> {
    writeln!(w, "genvert - generate random vertical text for encodevert")?;
    writeln!(w)?;
    writeln!(w, "Output format:")?;
    writeln!(w, "  - structure tags: <doc id=\"N\">, </doc>, <s>, </s>")?;
    writeln!(w, "  - token lines: WORD\\tPOS\\tLEMMA-POS")?;
    writeln!(w)?;
    writeln!(w, "Usage:")?;
    writeln!(
        w,
        "  genvert [--seed N] [--docs N] [--max-lines N] [--vocab N] [--zipf S]"
    )?;
    writeln!(w, "          [--word-len N] [--lemma-group N]")?;
    writeln!(w, "          [--min-sent N] [--max-sent N]")?;
    writeln!(w, "          [--min-doc-words N] [--max-doc-words N]")?;
    writeln!(w, "          [--cap-prob P] [--pos-tags STRING]")?;
    writeln!(w)?;
    writeln!(w, "Examples:")?;
    writeln!(w, "  genvert --seed 1 --docs 3 > sample.vert")?;
    writeln!(
        w,
        "  genvert --seed 1 --docs 999 --max-lines 100000 > sample.vert"
    )?;
    writeln!(
        w,
        "  genvert --seed 42 --vocab 5000 --min-doc-words 800 --max-doc-words 1600"
    )?;
    writeln!(w)?;
    writeln!(
        w,
        "Notes: --max-lines counts only token (tab-separated) lines; structure tags are not counted."
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = parse_args()?;
    validate(&cfg)?;

    let mut rng = SplitMix64::new(cfg.seed);

    let sampler = ZipfSampler::new(cfg.vocab, cfg.zipf_s)?;
    let vocab = Vocabulary::new(cfg.vocab, cfg.word_len, cfg.lemma_group, cfg.zipf_s)?;

    let mut out = io::BufWriter::new(io::stdout().lock());
    let mut tokens_emitted = 0usize;
    for doc_id in 0..cfg.docs {
        if !can_emit_more_tokens(&cfg, tokens_emitted, 1) {
            break;
        }
        writeln!(out, "<doc id=\"{}\">", doc_id)?;
        let target_words = rng.triangular_usize(cfg.min_doc_words, cfg.max_doc_words);
        let mut emitted = 0usize;
        let mut sentence_open = false;
        'doc: while emitted < target_words {
            if !can_emit_more_tokens(&cfg, tokens_emitted, 1) {
                break;
            }
            writeln!(out, "<s>")?;
            sentence_open = true;

            let remaining = target_words - emitted;
            let mut sent_len = rng.triangular_usize(cfg.min_sent_len, cfg.max_sent_len);
            sent_len = min(sent_len, remaining);
            sent_len = max(sent_len, cfg.min_sent_len.min(remaining));

            for i in 0..sent_len {
                if !can_emit_more_tokens(&cfg, tokens_emitted, 1) {
                    break 'doc;
                }
                let rank0 = sampler.sample_rank0(&mut rng);
                let word_lower = vocab.word_for_rank0(rank0);
                let mut word = word_lower.clone();

                let should_cap = i == 0 || rng.next_f64() < cfg.cap_prob;
                if should_cap {
                    capitalize_first_ascii(&mut word);
                }

                let pos = cfg.pos_tags[rng.gen_range_usize(0..cfg.pos_tags.len())];
                let lemma = strip_last_char(&word_lower);
                writeln!(out, "{}\t{}\t{}-{}", word, pos, lemma, pos)?;
                tokens_emitted += 1;
            }

            writeln!(out, "</s>")?;
            sentence_open = false;
            emitted += sent_len;
        }
        // If we broke out mid-sentence (typically due to token budget), close the sentence.
        if sentence_open {
            writeln!(out, "</s>")?;
        }
        writeln!(out, "</doc>")?;
        if !can_emit_more_tokens(&cfg, tokens_emitted, 1) {
            break;
        }
    }
    out.flush()?;
    Ok(())
}

fn parse_args() -> Result<Config, Box<dyn std::error::Error>> {
    let mut cfg = Config::default();
    let mut pargs = Arguments::from_env();
    if pargs.contains(["-h", "--help"]) {
        print_usage(io::stdout())?;
        std::process::exit(0);
    }
    if let Some(seed) = pargs.opt_value_from_str("--seed")? {
        cfg.seed = seed;
    }
    if let Some(docs) = pargs.opt_value_from_str("--docs")? {
        cfg.docs = docs;
    }
    if let Some(max_lines) = pargs.opt_value_from_str("--max-lines")? {
        cfg.max_lines = Some(max_lines);
    }
    if let Some(vocab) = pargs.opt_value_from_str("--vocab")? {
        cfg.vocab = vocab;
    }
    if let Some(zipf_s) = pargs.opt_value_from_str("--zipf")? {
        cfg.zipf_s = zipf_s;
    }
    if let Some(word_len) = pargs.opt_value_from_str("--word-len")? {
        cfg.word_len = word_len;
    }
    if let Some(lemma_group) = pargs.opt_value_from_str("--lemma-group")? {
        cfg.lemma_group = lemma_group;
    }
    if let Some(min_sent_len) = pargs.opt_value_from_str("--min-sent")? {
        cfg.min_sent_len = min_sent_len;
    }
    if let Some(max_sent_len) = pargs.opt_value_from_str("--max-sent")? {
        cfg.max_sent_len = max_sent_len;
    }
    if let Some(min_doc_words) = pargs.opt_value_from_str("--min-doc-words")? {
        cfg.min_doc_words = min_doc_words;
    }
    if let Some(max_doc_words) = pargs.opt_value_from_str("--max-doc-words")? {
        cfg.max_doc_words = max_doc_words;
    }
    if let Some(cap_prob) = pargs.opt_value_from_str("--cap-prob")? {
        cfg.cap_prob = cap_prob;
    }
    if let Some(pos_tags) = pargs.opt_value_from_str::<_, String>("--pos-tags")? {
        cfg.pos_tags = pos_tags.chars().collect();
    }
    if let Some(arg) = pargs.finish().into_iter().next() {
        return Err(format!("unknown arg: {}", arg.to_string_lossy()).into());
    }
    Ok(cfg)
}

fn validate(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if cfg.docs == 0 {
        return Err("--docs must be > 0".into());
    }
    if let Some(max_lines) = cfg.max_lines {
        if max_lines < 2 {
            return Err("--max-lines must be >= 2 (token lines only)".into());
        }
    }
    if cfg.vocab == 0 {
        return Err("--vocab must be > 0".into());
    }
    if !(cfg.zipf_s.is_finite() && cfg.zipf_s > 0.0) {
        return Err("--zipf must be a finite number > 0".into());
    }
    if cfg.word_len < 2 {
        return Err("--word-len must be >= 2 (lemma strips last character)".into());
    }
    if cfg.lemma_group == 0 || cfg.lemma_group > 26 {
        return Err("--lemma-group must be in 1..=26".into());
    }
    if cfg.min_sent_len == 0 || cfg.max_sent_len < cfg.min_sent_len {
        return Err("--min-sent must be > 0 and <= --max-sent".into());
    }
    if cfg.min_doc_words == 0 || cfg.max_doc_words < cfg.min_doc_words {
        return Err("--min-doc-words must be > 0 and <= --max-doc-words".into());
    }
    if !(cfg.cap_prob.is_finite() && (0.0..=1.0).contains(&cfg.cap_prob)) {
        return Err("--cap-prob must be in [0, 1]".into());
    }
    if cfg.pos_tags.len() != 10 {
        return Err("--pos-tags must be exactly 10 characters".into());
    }
    Ok(())
}

fn can_emit_more_tokens(cfg: &Config, tokens_emitted: usize, needed_more: usize) -> bool {
    match cfg.max_lines {
        None => true,
        Some(max_lines) => tokens_emitted.saturating_add(needed_more) <= max_lines,
    }
}

fn strip_last_char(s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    if !chars.is_empty() {
        chars.pop();
    }
    chars.into_iter().collect()
}

fn capitalize_first_ascii(s: &mut String) {
    if let Some(first) = s.as_bytes().first().copied() {
        if first.is_ascii_lowercase() {
            // Safe because we only change ASCII.
            let mut bytes = s.clone().into_bytes();
            bytes[0] = bytes[0].to_ascii_uppercase();
            *s = String::from_utf8(bytes).unwrap();
        }
    }
}

struct Vocabulary {
    stems: Vec<String>,
    suffixes: Vec<char>,
    stem_len: usize,
    lemma_group: usize,
}

impl Vocabulary {
    fn new(
        vocab: usize,
        word_len: usize,
        lemma_group: usize,
        zipf_s: f64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let stem_len = word_len - 1;
        let lemma_count = (vocab + lemma_group - 1) / lemma_group;
        let suffixes: Vec<char> = ('a'..='z').take(lemma_group).collect();
        if suffixes.len() != lemma_group {
            return Err("invalid --lemma-group".into());
        }

        let ln_w_min = -(vocab as f64).ln() * zipf_s;
        let mut stems = Vec::with_capacity(lemma_count);
        for lemma_id in 0..lemma_count {
            let rank = lemma_id * lemma_group + 1; // 1-based
            let ln_w = -(rank as f64).ln() * zipf_s; // ln(r^-s)
            // Normalize so rank=1 => score=1, rank=vocab => score=0
            let t = if ln_w_min == 0.0 {
                1.0
            } else {
                ln_w / ln_w_min
            };
            let score = (1.0 - t).clamp(0.0, 1.0);
            stems.push(encode_base26(score, stem_len));
        }

        Ok(Self {
            stems,
            suffixes,
            stem_len,
            lemma_group,
        })
    }

    fn word_for_rank0(&self, rank0: usize) -> String {
        let lemma_id = rank0 / self.lemma_group;
        let variant = rank0 % self.lemma_group;
        let mut s = String::with_capacity(self.stem_len + 1);
        s.push_str(&self.stems[lemma_id]);
        s.push(self.suffixes[variant]);
        s
    }
}

fn encode_base26(score: f64, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let mut maxv: u64 = 1;
    for _ in 0..len {
        maxv = maxv.saturating_mul(26);
    }
    let maxv = maxv.saturating_sub(1);
    let mut n = ((score.clamp(0.0, 1.0)) * (maxv as f64)).floor() as u64;

    let mut out = vec!['a'; len];
    for i in 0..len {
        let p = len - 1 - i;
        let div = 26u64.pow(p as u32);
        let d = (n / div) as u8;
        n %= div;
        out[i] = (b'a' + min(d, 25)) as char;
    }
    out.into_iter().collect()
}

struct ZipfSampler {
    cdf: Vec<f64>,
}

impl ZipfSampler {
    fn new(vocab: usize, s: f64) -> Result<Self, Box<dyn std::error::Error>> {
        if vocab == 0 {
            return Err("vocab must be > 0".into());
        }
        if !(s.is_finite() && s > 0.0) {
            return Err("zipf exponent must be finite and > 0".into());
        }
        let mut weights = Vec::with_capacity(vocab);
        let mut sum = 0.0f64;
        for r in 1..=vocab {
            let w = 1.0 / (r as f64).powf(s);
            sum += w;
            weights.push(sum);
        }
        for v in &mut weights {
            *v /= sum;
        }
        Ok(Self { cdf: weights })
    }

    fn sample_rank0(&self, rng: &mut SplitMix64) -> usize {
        let u = rng.next_f64();
        // partition_point returns first index where predicate is false.
        let idx = self.cdf.partition_point(|&p| p < u);
        min(idx, self.cdf.len() - 1)
    }
}

#[derive(Clone, Debug)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn next_f64(&mut self) -> f64 {
        // 53 random bits in [0, 1).
        let x = self.next_u64() >> 11;
        (x as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    fn gen_range_usize(&mut self, range: std::ops::Range<usize>) -> usize {
        let len = range.end - range.start;
        if len == 0 {
            return range.start;
        }
        // rejection sampling for modulo bias
        let zone = u64::MAX - (u64::MAX % (len as u64));
        loop {
            let v = self.next_u64();
            if v < zone {
                return range.start + (v % (len as u64)) as usize;
            }
        }
    }

    fn triangular_usize(&mut self, lo: usize, hi: usize) -> usize {
        if lo >= hi {
            return lo;
        }
        let span = (hi - lo) as f64;
        let t = (self.next_f64() + self.next_f64() + self.next_f64()) / 3.0;
        lo + (t * (span + 1.0)).floor() as usize
    }
}
