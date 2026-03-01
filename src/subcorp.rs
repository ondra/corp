use fs_err::File;
use memmap::MmapOptions;
use std::sync::OnceLock;

use crate::corp::{Attr, Corpus, CorpusLike, Frequency, open_freq};
use crate::rev;
use crate::structure;
use crate::text;
use crate::util::as_slice_ref;

pub struct Ranges {
    map: memmap::Mmap,
    search_size_cache: OnceLock<u64>,
}

impl Ranges {
    pub fn open(path: &str) -> Result<Ranges, Box<dyn std::error::Error>> {
        let f = File::open(path)?;
        let map = unsafe { MmapOptions::new().map(f.file())? };
        Ok(Ranges {
            map,
            search_size_cache: OnceLock::new(),
        })
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

    /// Total number of positions covered by the subcorpus.
    /// Cached after first computation.
    pub fn search_size(&self) -> u64 {
        *self
            .search_size_cache
            .get_or_init(|| self.pairs().iter().map(|&(b, e)| e.saturating_sub(b)).sum())
    }

    /// Compute evenly spaced worker starts as absolute corpus positions.
    /// Uses two passes over ranges and O(threads) memory.
    pub fn compute_start_positions(&self, threads: usize) -> Vec<u64> {
        if threads == 0 {
            return Vec::new();
        }

        let pairs = self.pairs();
        if pairs.is_empty() {
            return vec![0u64; threads];
        }

        let search_size = self.search_size();
        if search_size == 0 {
            return vec![pairs[0].0; threads];
        }

        let targets: Vec<u64> = (0..threads)
            .map(|i| ((i as u128 * search_size as u128) / threads as u128) as u64)
            .collect();

        let mut starts = vec![pairs[0].0; threads];
        let mut seen = 0u64;
        let mut ti = 0usize;
        for &(beg, end) in pairs {
            let len = end.saturating_sub(beg);
            while ti < targets.len() && targets[ti] < seen.saturating_add(len) {
                starts[ti] = beg + (targets[ti] - seen);
                ti += 1;
            }
            seen = seen.saturating_add(len);
            if ti >= targets.len() {
                break;
            }
        }
        starts
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

#[derive(Debug)]
pub struct SubCorpusStructure<'a> {
    inner: &'a dyn structure::Struct,
    ranges: &'a Ranges,
}

impl<'a> SubCorpusStructure<'a> {
    pub fn new(inner: &'a dyn structure::Struct, ranges: &'a Ranges) -> Self {
        SubCorpusStructure { inner, ranges }
    }

    pub fn inner(&self) -> &'a dyn structure::Struct {
        self.inner
    }

    pub fn ranges(&self) -> &'a Ranges {
        self.ranges
    }

    pub fn contains_range(&self, beg: u64, end: u64) -> bool {
        self.ranges.contains_range(beg, end)
    }

    pub fn filtered_iter(&'a self) -> impl Iterator<Item = (u64, u64)> + 'a {
        filtered_struct_iter(self.inner, self.ranges)
    }
}

#[derive(Debug)]
pub struct SubCorpus<'a> {
    pub corpus: &'a Corpus,
    pub ranges: Ranges,
    subcorp_base: String,
}

impl<'a> SubCorpus<'a> {
    /*
    /// Open from "corpname:subcpath" spec.
    pub fn open(spec: &str) -> Result<SubCorpus, Box<dyn std::error::Error>> {
        let (corpname, subcpath) = spec.split_once(':')
            .ok_or_else(|| format!("subcorpus spec must be corpname:subcpath, got '{}'", spec))?;
        let corpus = Corpus::open(corpname)?;
        SubCorpus::from_corpus(&corpus, subcpath)
    }
    */

    /// Layer subcorpus over an already-opened corpus.
    pub fn from_corpus(
        corpus: &'a Corpus,
        subcpath: &str,
    ) -> Result<SubCorpus<'a>, Box<dyn std::error::Error>> {
        let ranges = Ranges::open(subcpath)?;
        let subcorp_base = subcpath
            .strip_suffix("subc")
            .unwrap_or(subcpath)
            .to_string();
        Ok(SubCorpus {
            corpus,
            ranges,
            subcorp_base,
        })
    }

    pub fn ranges(&self) -> &Ranges {
        &self.ranges
    }
}

pub fn resolve_subc_path(corpus: &Corpus, subcpath: &str) -> String {
    let p = std::path::Path::new(subcpath);
    if p.is_absolute() || p.exists() {
        subcpath.to_string()
    } else {
        std::path::Path::new(&corpus.path)
            .join(subcpath)
            .to_string_lossy()
            .to_string()
    }
}

/// Open `CORPUS` or `CORPUS[:SUBCORPUS]` and call `f` with the resulting CorpusLike.
pub fn with_corpuslike_spec<R, F>(spec: &str, f: F) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(&dyn CorpusLike) -> Result<R, Box<dyn std::error::Error>>,
{
    let (corpname, subcpath) = match spec.split_once(':') {
        Some((corpname, subcpath)) => (corpname, Some(subcpath)),
        None => (spec, None),
    };
    if corpname.is_empty() {
        return Err(format!("missing corpus name in corpus spec '{spec}'").into());
    }

    let corpus = Box::new(Corpus::open(corpname)?);
    if let Some(subcpath) = subcpath {
        if subcpath.is_empty() {
            return Err(format!("missing subcorpus path in corpus spec '{spec}'").into());
        }
        let subcpath = resolve_subc_path(corpus.as_ref(), subcpath);
        let subcorp = SubCorpus::from_corpus(corpus.as_ref(), &subcpath)?;
        f(&subcorp as &dyn CorpusLike)
    } else {
        f(corpus.as_ref() as &dyn CorpusLike)
    }
}

impl<'a> CorpusLike for SubCorpus<'a> {
    fn open_attribute(
        &self,
        name: &str,
    ) -> Result<Box<dyn Attr + Sync + Send + '_>, Box<dyn std::error::Error>> {
        let inner = self.corpus.open_attribute(name)?;
        let freq_base = self.subcorp_base.clone() + name;
        Ok(Box::new(SubCorpAttr {
            inner,
            ranges: &self.ranges,
            freq_base,
        }))
    }

    fn open_struct(
        &self,
        name: &str,
    ) -> Result<Box<dyn structure::Struct + Sync + Send + '_>, Box<dyn std::error::Error>> {
        self.corpus.open_struct(name)
    }

    fn get_conf(&self, name: &str) -> Option<String> {
        self.corpus.get_conf(name)
    }

    fn search_size(&self) -> u64 {
        self.ranges.search_size()
    }

    fn subcorp(&self) -> Option<&Ranges> {
        Some(&self.ranges)
    }
}

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
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(SubCorpIdIter::new(&*self.inner, &self.ranges, frompos))
    }

    fn id2str(&self, id: u32) -> &str {
        self.inner.id2str(id)
    }
    fn str2id(&self, s: &str) -> Option<u32> {
        self.inner.str2id(s)
    }
    fn id_range(&self) -> u32 {
        self.inner.id_range()
    }

    fn revidx(&self) -> &dyn rev::Rev {
        self
    }
    fn text(&self) -> &dyn text::Text {
        self
    }

    fn get_freq(
        &self,
        t: &str,
    ) -> Result<Box<dyn Frequency + Send + Sync + '_>, Box<dyn std::error::Error>> {
        open_freq(&self.freq_base, t)
    }
}

impl rev::Rev for SubCorpAttr<'_> {
    fn count(&self, id: u32) -> u64 {
        self.id2poss(id).count() as u64
    }

    fn id2poss(&self, id: u32) -> Box<dyn Iterator<Item = u64> + Send + Sync + '_> {
        let inner_poss = self.inner.revidx().id2poss(id);
        Box::new(FilteredRevIter {
            inner: inner_poss,
            pairs: self.ranges.pairs(),
            ri: 0,
        })
    }
}

impl text::Text for SubCorpAttr<'_> {
    fn size(&self) -> usize {
        self.inner.text().size() as usize
    }
    fn get(&self, pos: u64) -> u32 {
        self.inner.text().get(pos)
    }
    fn posat(&self, _pos: u64) -> Option<text::DeltaIter<'_>> {
        None
    }
    fn structat(&self, _pos: u64) -> Option<text::IntIter<'_>> {
        None
    }
}

/// Walks through active ranges, yielding IDs only from positions within ranges.
struct SubCorpIdIter<'a> {
    inner: &'a dyn Attr,
    ranges: &'a Ranges,
    ri: usize,
    cur_iter: Box<dyn Iterator<Item = u32> + 'a>,
    remaining_in_range: u64,
}

impl<'a> SubCorpIdIter<'a> {
    fn new(inner: &'a dyn Attr, ranges: &'a Ranges, frompos: u64) -> Self {
        let pairs = ranges.pairs();
        let ri = ranges.first_range_at_or_after(frompos);
        if ri >= pairs.len() {
            return SubCorpIdIter {
                inner,
                ranges,
                ri,
                cur_iter: Box::new(std::iter::empty()),
                remaining_in_range: 0,
            };
        }
        let (rbeg, rend) = pairs[ri];
        let start = rbeg.max(frompos);
        let cur_iter = inner.iter_ids(start);
        SubCorpIdIter {
            inner,
            ranges,
            ri,
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

/// Lazy merge-intersect of inner position iterator with ranges.
struct FilteredRevIter<'a> {
    inner: Box<dyn Iterator<Item = u64> + Send + Sync + 'a>,
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
                    if self.ri < self.pairs.len()
                        && self.pairs[self.ri].0 <= pos
                        && pos < self.pairs[self.ri].1
                    {
                        return Some(pos);
                    }
                    // pos is in a gap, continue
                }
            }
        }
    }
}

/// Iterate all structures as (beg, end) pairs.
pub fn struct_iter(s: &dyn structure::Struct) -> impl Iterator<Item = (u64, u64)> + '_ {
    struct_iter_from_pos(s, 0)
}

pub fn struct_index_at_or_after_pos(s: &dyn structure::Struct, pos: u64) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    if let Some(idx) = s.num_at_pos(pos) {
        return Some(idx);
    }
    let (idx, _) = s.find_end(pos.saturating_add(1));
    if idx == u64::MAX { None } else { Some(idx) }
}

struct StructIterFromPos<'a> {
    s: &'a dyn structure::Struct,
    next_idx: u64,
    end_idx: u64,
}

impl Iterator for StructIterFromPos<'_> {
    type Item = (u64, u64);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_idx >= self.end_idx {
            return None;
        }
        let i = self.next_idx;
        self.next_idx += 1;
        Some((self.s.beg_at(i), self.s.end_at(i)))
    }
}

/// Iterate structures as (beg, end) pairs, starting at the first structure at/after from_pos.
pub fn struct_iter_from_pos(
    s: &dyn structure::Struct,
    from_pos: u64,
) -> impl Iterator<Item = (u64, u64)> + '_ {
    let next_idx = struct_index_at_or_after_pos(s, from_pos).unwrap_or(s.len() as u64);
    StructIterFromPos {
        s,
        next_idx,
        end_idx: s.len() as u64,
    }
}

struct FilteredStructIterFromPos<'a> {
    s: &'a dyn structure::Struct,
    pairs: &'a [(u64, u64)],
    ri: usize,
    doc_idx: u64,
    start_pos: u64,
    first_range: bool,
}

impl FilteredStructIterFromPos<'_> {
    fn seek_current_range_start(&mut self) {
        if self.ri >= self.pairs.len() {
            self.doc_idx = u64::MAX;
            return;
        }
        let (rbeg, _rend) = self.pairs[self.ri];
        let scan_from = if self.first_range {
            self.first_range = false;
            rbeg.max(self.start_pos)
        } else {
            rbeg
        };
        self.doc_idx = struct_index_at_or_after_pos(self.s, scan_from).unwrap_or(u64::MAX);
    }
}

impl Iterator for FilteredStructIterFromPos<'_> {
    type Item = (u64, u64);

    fn next(&mut self) -> Option<Self::Item> {
        let struct_len = self.s.len() as u64;
        loop {
            if self.ri >= self.pairs.len() || self.doc_idx == u64::MAX {
                return None;
            }
            let (rbeg, rend) = self.pairs[self.ri];
            while self.doc_idx < struct_len {
                let i = self.doc_idx;
                let beg = self.s.beg_at(i);
                if beg >= rend {
                    break;
                }
                let end = self.s.end_at(i);
                self.doc_idx += 1;
                if rbeg <= beg && end <= rend {
                    return Some((beg, end));
                }
            }
            self.ri += 1;
            self.seek_current_range_start();
        }
    }
}

/// Iterate only structures fully contained within some range.
pub fn filtered_struct_iter<'a>(
    s: &'a dyn structure::Struct,
    ranges: &'a Ranges,
) -> impl Iterator<Item = (u64, u64)> + 'a {
    filtered_struct_iter_from_pos(s, ranges, 0)
}

/// Iterate only structures fully contained within ranges, starting at from_pos.
/// Iteration walks ranges first, which is efficient for sparse subcorpora.
pub fn filtered_struct_iter_from_pos<'a>(
    s: &'a dyn structure::Struct,
    ranges: &'a Ranges,
    from_pos: u64,
) -> impl Iterator<Item = (u64, u64)> + 'a {
    let mut it = FilteredStructIterFromPos {
        s,
        pairs: ranges.pairs(),
        ri: ranges.first_range_at_or_after(from_pos),
        doc_idx: u64::MAX,
        start_pos: from_pos,
        first_range: true,
    };
    it.seek_current_range_start();
    it
}

/// Count the number of structures fully contained within the ranges.
pub fn count_filtered_structs(s: &dyn structure::Struct, ranges: &Ranges) -> usize {
    filtered_struct_iter(s, ranges).count()
}
