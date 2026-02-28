use fs_err::File;
use memmap::MmapOptions;

use crate::corp::{Attr, Corpus, CorpusLike, Frequency, open_freq};
use crate::rev;
use crate::text;
use crate::structure;
use crate::util::as_slice_ref;

// --- Ranges ---

pub struct Ranges {
    map: memmap::Mmap,
}

impl Ranges {
    pub fn open(path: &str) -> Result<Ranges, Box<dyn std::error::Error>> {
        let f = File::open(path)?;
        let map = unsafe { MmapOptions::new().map(f.file())? };
        Ok(Ranges { map })
    }

    pub fn pairs(&self) -> &[(u64, u64)] {
        as_slice_ref(&self.map)
    }

    pub fn len(&self) -> usize {
        self.map.len() / 16
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total_positions(&self) -> u64 {
        self.pairs().iter().map(|&(b, e)| e - b).sum()
    }

    /// Binary search: true if pos is within some range [beg, end).
    pub fn contains(&self, pos: u64) -> bool {
        let pairs = self.pairs();
        let mut lo = 0usize;
        let mut hi = pairs.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if pairs[mid].1 <= pos {
                lo = mid + 1;
            } else if pairs[mid].0 > pos {
                hi = mid;
            } else {
                return true;
            }
        }
        false
    }

    /// True if the entire [beg, end) is fully contained within some single range.
    pub fn contains_range(&self, beg: u64, end: u64) -> bool {
        let pairs = self.pairs();
        // Find the last range whose start <= beg
        let mut lo = 0usize;
        let mut hi = pairs.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if pairs[mid].0 <= beg {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // lo is one past the last range with start <= beg
        if lo == 0 {
            return false;
        }
        let idx = lo - 1;
        pairs[idx].0 <= beg && end <= pairs[idx].1
    }

    /// Index of the first range whose end > pos, i.e. the first range at or after pos.
    pub fn first_range_at_or_after(&self, pos: u64) -> usize {
        let pairs = self.pairs();
        let mut lo = 0usize;
        let mut hi = pairs.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if pairs[mid].1 <= pos {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

impl std::fmt::Debug for Ranges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ranges").field("len", &self.len()).finish()
    }
}

// --- SubCorpus ---

#[derive(Debug)]
pub struct SubCorpus {
    pub corpus: Corpus,
    pub ranges: Ranges,
    subcorp_base: String,
}

impl SubCorpus {
    /// Open from "corpname:subcpath" spec.
    pub fn open(spec: &str) -> Result<SubCorpus, Box<dyn std::error::Error>> {
        let (corpname, subcpath) = spec.split_once(':')
            .ok_or_else(|| format!("subcorpus spec must be corpname:subcpath, got '{}'", spec))?;
        let corpus = Corpus::open(corpname)?;
        SubCorpus::from_corpus(corpus, subcpath)
    }

    /// Layer subcorpus over an already-opened corpus.
    pub fn from_corpus(corpus: Corpus, subcpath: &str) -> Result<SubCorpus, Box<dyn std::error::Error>> {
        let ranges = Ranges::open(subcpath)?;
        // Strip literal "subc" suffix from path to get freq base
        let subcorp_base = subcpath.strip_suffix("subc")
            .unwrap_or(subcpath)
            .to_string();
        Ok(SubCorpus { corpus, ranges, subcorp_base })
    }

    pub fn ranges(&self) -> &Ranges {
        &self.ranges
    }
}

impl CorpusLike for SubCorpus {
    fn open_attribute(&self, name: &str) -> Result<Box<dyn Attr + Sync + Send + '_>, Box<dyn std::error::Error>> {
        let inner = self.corpus.open_attribute(name)?;
        let freq_base = self.subcorp_base.clone() + name;
        Ok(Box::new(SubCorpAttr {
            inner,
            ranges: &self.ranges,
            freq_base,
        }))
    }

    fn open_struct(&self, name: &str) -> Result<Box<dyn structure::Struct + Sync + Send + '_>, Box<dyn std::error::Error>> {
        self.corpus.open_struct(name)
    }

    fn get_conf(&self, name: &str) -> Option<String> {
        self.corpus.get_conf(name)
    }
}

// --- SubCorpAttr ---

struct SubCorpAttr<'a> {
    inner: Box<dyn Attr + Sync + Send + 'a>,
    ranges: &'a Ranges,
    freq_base: String,
}

impl std::fmt::Debug for SubCorpAttr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubCorpAttr")
            .field("inner", &self.inner)
            .field("freq_base", &self.freq_base)
            .finish()
    }
}

impl Attr for SubCorpAttr<'_> {
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item=u32> + '_> {
        Box::new(SubCorpIdIter::new(&*self.inner, &self.ranges, frompos))
    }

    fn id2str(&self, id: u32) -> &str { self.inner.id2str(id) }
    fn str2id(&self, s: &str) -> Option<u32> { self.inner.str2id(s) }
    fn id_range(&self) -> u32 { self.inner.id_range() }

    fn revidx(&self) -> &dyn rev::Rev { self }
    fn text(&self) -> &dyn text::Text { self }

    fn get_freq(&self, t: &str) -> Result<Box<dyn Frequency + Send + Sync + '_>, Box<dyn std::error::Error>> {
        if t == "frq" {
            return open_freq(&self.freq_base, "frq").map_err(|_| {
                std::io::Error::other(format!(
                    "missing precomputed subcorpus frequency: {}.frq/.frq64",
                    self.freq_base
                ))
                .into()
            });
        }
        open_freq(&self.freq_base, t)
    }
}

// --- SubCorpAttr as Rev ---

impl rev::Rev for SubCorpAttr<'_> {
    fn count(&self, id: u32) -> u64 {
        self.id2poss(id).count() as u64
    }

    fn id2poss(&self, id: u32) -> Box<dyn Iterator<Item=u64> + Send + Sync + '_> {
        let inner_poss = self.inner.revidx().id2poss(id);
        Box::new(FilteredRevIter {
            inner: inner_poss,
            pairs: self.ranges.pairs(),
            ri: 0,
        })
    }
}

// --- SubCorpAttr as Text ---

impl text::Text for SubCorpAttr<'_> {
    fn size(&self) -> usize { self.ranges.total_positions() as usize }
    fn get(&self, pos: u64) -> u32 { self.inner.text().get(pos) }
    fn posat(&self, _pos: u64) -> Option<text::DeltaIter<'_>> { None }
    fn structat(&self, _pos: u64) -> Option<text::IntIter<'_>> { None }
}


// --- SubCorpIdIter ---

/// Walks through active ranges, yielding IDs only from positions within ranges.
struct SubCorpIdIter<'a> {
    inner: &'a dyn Attr,
    ranges: &'a Ranges,
    ri: usize,
    cur_iter: Box<dyn Iterator<Item=u32> + 'a>,
    remaining_in_range: u64,
}

impl<'a> SubCorpIdIter<'a> {
    fn new(inner: &'a dyn Attr, ranges: &'a Ranges, frompos: u64) -> Self {
        let pairs = ranges.pairs();
        let ri = ranges.first_range_at_or_after(frompos);
        if ri >= pairs.len() {
            return SubCorpIdIter {
                inner, ranges, ri,
                cur_iter: Box::new(std::iter::empty()),
                remaining_in_range: 0,
            };
        }
        let (rbeg, rend) = pairs[ri];
        let start = rbeg.max(frompos);
        let cur_iter = inner.iter_ids(start);
        SubCorpIdIter {
            inner, ranges, ri,
            cur_iter,
            remaining_in_range: rend - start,
        }
    }
}

impl Iterator for SubCorpIdIter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        loop {
            if self.remaining_in_range > 0 {
                if let Some(id) = self.cur_iter.next() {
                    self.remaining_in_range -= 1;
                    return Some(id);
                }
            }
            // Move to next range
            self.ri += 1;
            let pairs = self.ranges.pairs();
            if self.ri >= pairs.len() {
                return None;
            }
            let (rbeg, rend) = pairs[self.ri];
            self.cur_iter = self.inner.iter_ids(rbeg);
            self.remaining_in_range = rend - rbeg;
        }
    }
}

// --- FilteredRevIter ---

/// Lazy merge-intersect of inner position iterator with ranges.
struct FilteredRevIter<'a> {
    inner: Box<dyn Iterator<Item=u64> + Send + Sync + 'a>,
    pairs: &'a [(u64, u64)],
    ri: usize,
}

impl Iterator for FilteredRevIter<'_> {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        loop {
            if self.ri >= self.pairs.len() {
                return None;
            }
            let (rbeg, rend) = self.pairs[self.ri];
            match self.inner.next() {
                None => return None,
                Some(pos) => {
                    if pos < rbeg {
                        // Position before current range, skip
                        continue;
                    }
                    if pos < rend {
                        return Some(pos);
                    }
                    // pos >= rend, advance ranges
                    self.ri += 1;
                    while self.ri < self.pairs.len() && self.pairs[self.ri].1 <= pos {
                        self.ri += 1;
                    }
                    if self.ri < self.pairs.len() && self.pairs[self.ri].0 <= pos && pos < self.pairs[self.ri].1 {
                        return Some(pos);
                    }
                    // pos is in a gap, continue
                }
            }
        }
    }
}


// --- Structure iteration helpers ---

/// Iterate all structures as (beg, end) pairs.
pub fn struct_iter(s: &dyn structure::Struct) -> impl Iterator<Item=(u64, u64)> + '_ {
    (0..s.len() as u64).map(move |i| (s.beg_at(i), s.end_at(i)))
}

/// Iterate only structures fully contained within some range.
pub fn filtered_struct_iter<'a>(
    s: &'a dyn structure::Struct,
    ranges: &'a Ranges,
) -> impl Iterator<Item=(u64, u64)> + 'a {
    struct_iter(s).filter(|&(beg, end)| ranges.contains_range(beg, end))
}

/// Count the number of structures fully contained within the ranges.
pub fn count_filtered_structs(s: &dyn structure::Struct, ranges: &Ranges) -> usize {
    filtered_struct_iter(s, ranges).count()
}
