use fs_err::File;
use memmap::MmapOptions;
use std::io::Read;

use crate::corp::{Attr, Corpus, Frequency, open_freq};
use crate::lex;
use crate::rev;
use crate::structure;
use crate::text;
use crate::util::as_slice_ref;

pub fn parse_virtdef(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut f = File::open(path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;

    let mut names = Vec::new();
    let mut lines = buf.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('=') {
            let range_line = lines
                .next()
                .ok_or_else(|| format!("expected range after ={}", name))?
                .trim();
            if range_line != "0,$" {
                return Err(
                    format!("unsupported range '{}' for segment {}", range_line, name).into(),
                );
            }
            names.push(name.to_string());
        } else {
            return Err(format!("unexpected line in virtdef: {}", line).into());
        }
    }
    Ok(names)
}

#[derive(Debug)]
pub struct SegmentLayout {
    /// cumulative_sizes[i] = sum of text sizes of segments 0..i
    /// Segment i spans virtual positions [cumulative_sizes[i], cumulative_sizes[i+1])
    /// Length = num_segments + 1
    pub cumulative_sizes: Vec<u64>,
}

impl SegmentLayout {
    pub fn new(sizes: &[u64]) -> SegmentLayout {
        let mut cumulative = Vec::with_capacity(sizes.len() + 1);
        cumulative.push(0);
        let mut sum = 0u64;
        for &s in sizes {
            sum += s;
            cumulative.push(sum);
        }
        SegmentLayout {
            cumulative_sizes: cumulative,
        }
    }

    pub fn num_segments(&self) -> usize {
        self.cumulative_sizes.len() - 1
    }

    pub fn total_size(&self) -> u64 {
        *self.cumulative_sizes.last().unwrap()
    }

    /// Returns (segment_index, local_position)
    pub fn locate(&self, vpos: u64) -> (usize, u64) {
        // binary search: find largest i such that cumulative_sizes[i] <= vpos
        let cs = &self.cumulative_sizes;
        let mut lo = 0usize;
        let mut hi = cs.len() - 1; // last valid segment boundary
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if cs[mid] <= vpos {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        (lo, vpos - cs[lo])
    }

    pub fn offset(&self, seg: usize) -> u64 {
        self.cumulative_sizes[seg]
    }

    pub fn seg_size(&self, seg: usize) -> u64 {
        self.cumulative_sizes[seg + 1] - self.cumulative_sizes[seg]
    }
}

#[derive(Debug)]
pub struct StructLayout {
    pub cumulative_counts: Vec<u64>,
}

impl StructLayout {
    pub fn new(counts: &[u64]) -> StructLayout {
        let mut cumulative = Vec::with_capacity(counts.len() + 1);
        cumulative.push(0);
        let mut sum = 0u64;
        for &c in counts {
            sum += c;
            cumulative.push(sum);
        }
        StructLayout {
            cumulative_counts: cumulative,
        }
    }

    pub fn total_count(&self) -> u64 {
        *self.cumulative_counts.last().unwrap()
    }

    /// Returns (segment_index, local_struct_pos)
    pub fn locate(&self, structpos: u64) -> (usize, u64) {
        let cs = &self.cumulative_counts;
        let mut lo = 0usize;
        let mut hi = cs.len() - 1;
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if cs[mid] <= structpos {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        (lo, structpos - cs[lo])
    }

    pub fn offset(&self, seg: usize) -> u64 {
        self.cumulative_counts[seg]
    }
}

fn open_mmap(path: &str) -> Result<memmap::Mmap, Box<dyn std::error::Error>> {
    let f = File::open(path)?;
    Ok(unsafe { MmapOptions::new().map(f.file())? })
}

fn nid_lookup(mmap: &memmap::Mmap, local_id: u32) -> u32 {
    as_slice_ref::<u32>(mmap)[local_id as usize]
}

fn oid_lookup(mmap: &memmap::Mmap, unified_id: u32) -> u32 {
    as_slice_ref::<u32>(mmap)[unified_id as usize]
}

#[derive(Debug)]
pub struct VirtualRev<'a> {
    layout: &'a SegmentLayout,
    seg_revs: Vec<Box<dyn rev::Rev + Sync + Send>>,
    oid: Vec<memmap::Mmap>,
}

impl rev::Rev for VirtualRev<'_> {
    fn count(&self, id: u32) -> u64 {
        let mut total = 0u64;
        for (i, seg_rev) in self.seg_revs.iter().enumerate() {
            let seg_id = oid_lookup(&self.oid[i], id);
            if seg_id != 0xFFFFFFFF {
                total += seg_rev.count(seg_id);
            }
        }
        total
    }

    fn id2poss(&self, id: u32) -> Box<dyn Iterator<Item = u64> + Send + Sync + '_> {
        Box::new(VirtualRevIter::new(self, id))
    }
}

struct VirtualRevIter<'iter, 'layout> {
    rev: &'iter VirtualRev<'layout>,
    id: u32,
    seg_idx: usize,
    cur_offset: u64,
    cur_iter: Option<Box<dyn Iterator<Item = u64> + Send + Sync + 'iter>>,
}

impl<'iter, 'layout> VirtualRevIter<'iter, 'layout> {
    fn new(rev: &'iter VirtualRev<'layout>, id: u32) -> Self {
        Self {
            rev,
            id,
            seg_idx: 0,
            cur_offset: 0,
            cur_iter: None,
        }
    }
}

impl Iterator for VirtualRevIter<'_, '_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(iter) = self.cur_iter.as_mut() {
                if let Some(pos) = iter.next() {
                    return Some(self.cur_offset + pos);
                }
            }
            if self.seg_idx >= self.rev.seg_revs.len() {
                return None;
            }
            let seg = self.seg_idx;
            self.seg_idx += 1;
            let seg_id = oid_lookup(&self.rev.oid[seg], self.id);
            if seg_id == 0xFFFFFFFF {
                self.cur_iter = None;
                continue;
            }
            self.cur_offset = self.rev.layout.offset(seg);
            self.cur_iter = Some(self.rev.seg_revs[seg].id2poss(seg_id));
        }
    }
}

#[derive(Debug)]
pub struct VirtualAttr<'a> {
    pub path: String,
    pub name: String,
    layout: &'a SegmentLayout,
    segments: Vec<Box<dyn Attr + Sync + Send + 'a>>,
    lex: lex::MapLex,
    nid: Vec<memmap::Mmap>,
    vrev: VirtualRev<'a>,
}

impl<'a> VirtualAttr<'a> {
    pub fn open(
        virt_path: &str,
        attr_name: &str,
        layout: &'a SegmentLayout,
        segment_corpora: &'a [Corpus],
    ) -> Result<VirtualAttr<'a>, Box<dyn std::error::Error>> {
        let base = virt_path.to_string() + "/" + attr_name;
        let nseg = segment_corpora.len();

        let mut segments = Vec::with_capacity(nseg);
        let mut nid_maps = Vec::with_capacity(nseg);
        let mut seg_revs: Vec<Box<dyn rev::Rev + Sync + Send>> = Vec::with_capacity(nseg);
        let mut rev_oid_maps = Vec::with_capacity(nseg);

        for (i, corp) in segment_corpora.iter().enumerate() {
            let seg_attr = corp.open_attribute(attr_name)?;
            nid_maps.push(open_mmap(&format!("{}.seg{}.nid", base, i))?);
            rev_oid_maps.push(open_mmap(&format!("{}.seg{}.oid", base, i))?);
            seg_revs.push(rev::open(&(corp.path.clone() + "/" + attr_name))?);
            segments.push(seg_attr);
        }

        let lex = lex::MapLex::open(&base)?;

        let vrev = VirtualRev {
            layout,
            seg_revs,
            oid: rev_oid_maps,
        };

        Ok(VirtualAttr {
            path: base,
            name: attr_name.to_string(),
            layout,
            segments,
            lex,
            nid: nid_maps,
            vrev,
        })
    }
}

// Iterator that chains across segments, translating IDs via nid
struct VirtualIdIter<'a> {
    layout: &'a SegmentLayout,
    segments: &'a [Box<dyn Attr + Sync + Send + 'a>],
    nid: &'a [memmap::Mmap],
    cur_seg: usize,
    cur_iter: Box<dyn Iterator<Item = u32> + 'a>,
    remaining_in_seg: u64,
}

impl<'a> VirtualIdIter<'a> {
    fn new(
        layout: &'a SegmentLayout,
        segments: &'a [Box<dyn Attr + Sync + Send + 'a>],
        nid: &'a [memmap::Mmap],
        frompos: u64,
    ) -> Self {
        let (seg, local) = layout.locate(frompos);
        let remaining = if seg < layout.num_segments() {
            layout.seg_size(seg) - local
        } else {
            0
        };
        let cur_iter: Box<dyn Iterator<Item = u32> + 'a> = if seg < segments.len() {
            segments[seg].iter_ids(local)
        } else {
            Box::new(std::iter::empty())
        };
        VirtualIdIter {
            layout,
            segments,
            nid,
            cur_seg: seg,
            cur_iter,
            remaining_in_seg: remaining,
        }
    }
}

impl Iterator for VirtualIdIter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        loop {
            if self.remaining_in_seg > 0 {
                if let Some(local_id) = self.cur_iter.next() {
                    self.remaining_in_seg -= 1;
                    return Some(nid_lookup(&self.nid[self.cur_seg], local_id));
                }
            }
            // Move to next segment
            self.cur_seg += 1;
            if self.cur_seg >= self.layout.num_segments() {
                return None;
            }
            self.remaining_in_seg = self.layout.seg_size(self.cur_seg);
            self.cur_iter = self.segments[self.cur_seg].iter_ids(0);
        }
    }
}

impl text::Text for VirtualAttr<'_> {
    fn posat(&self, _pos: u64) -> Option<text::DeltaIter<'_>> {
        None
    }
    fn structat(&self, _pos: u64) -> Option<text::IntIter<'_>> {
        None
    }
    fn size(&self) -> usize {
        self.layout.total_size() as usize
    }
    fn get(&self, pos: u64) -> u32 {
        let (seg, local) = self.layout.locate(pos);
        let local_id = self.segments[seg].text().get(local);
        nid_lookup(&self.nid[seg], local_id)
    }
}

impl Attr for VirtualAttr<'_> {
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(VirtualIdIter::new(
            &self.layout,
            &self.segments,
            &self.nid,
            frompos,
        ))
    }

    fn id2str(&self, id: u32) -> &str {
        self.lex.id2str(id)
    }
    fn str2id(&self, s: &str) -> Option<u32> {
        self.lex.str2id(s)
    }
    fn id_range(&self) -> u32 {
        self.lex.id_range()
    }

    fn revidx(&self) -> &dyn rev::Rev {
        &self.vrev
    }
    fn text(&self) -> &dyn text::Text {
        self
    }

    fn get_freq(
        &self,
        t: &str,
    ) -> Result<Box<dyn Frequency + Send + Sync + '_>, Box<dyn std::error::Error>> {
        if let Ok(freq) = open_freq(&self.path, t) {
            return Ok(freq);
        }
        let seg_freqs: Result<Vec<_>, _> = self.segments.iter().map(|s| s.get_freq(t)).collect();
        Ok(Box::new(VirtualFrequency {
            oid: &self.vrev.oid,
            seg_freqs: seg_freqs?,
        }))
    }
}

struct VirtualFrequency<'a> {
    oid: &'a [memmap::Mmap],
    seg_freqs: Vec<Box<dyn Frequency + Send + Sync + 'a>>,
}

impl Frequency for VirtualFrequency<'_> {
    fn frq(&self, id: u32) -> u64 {
        self.seg_freqs
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let seg_id = oid_lookup(&self.oid[i], id);
                if seg_id != 0xFFFFFFFF {
                    f.frq(seg_id)
                } else {
                    0
                }
            })
            .sum()
    }
}

#[derive(Debug)]
pub struct VirtualStruct<'a> {
    corp_layout: &'a SegmentLayout,
    struct_layout: StructLayout,
    segments: Vec<Box<dyn structure::Struct + Sync + Send + 'a>>,
}

impl<'a> VirtualStruct<'a> {
    pub fn open(
        corp_layout: &'a SegmentLayout,
        segment_corpora: &'a [Corpus],
        struct_name: &str,
    ) -> Result<VirtualStruct<'a>, Box<dyn std::error::Error>> {
        let mut segments: Vec<Box<dyn structure::Struct + Sync + Send + 'a>> = Vec::new();
        let mut counts = Vec::new();

        for corp in segment_corpora {
            let s = corp.open_struct(struct_name)?;
            counts.push(s.len() as u64);
            segments.push(s);
        }

        Ok(VirtualStruct {
            corp_layout,
            struct_layout: StructLayout::new(&counts),
            segments,
        })
    }
}

impl structure::Struct for VirtualStruct<'_> {
    fn beg_at(&self, pos: u64) -> u64 {
        let (seg, local) = self.struct_layout.locate(pos);
        self.segments[seg].beg_at(local) + self.corp_layout.offset(seg)
    }

    fn end_at(&self, pos: u64) -> u64 {
        let (seg, local) = self.struct_layout.locate(pos);
        self.segments[seg].end_at(local) + self.corp_layout.offset(seg)
    }

    fn len(&self) -> usize {
        self.struct_layout.total_count() as usize
    }

    fn num_at_pos(&self, pos: u64) -> Option<u64> {
        let (seg, local_pos) = self.corp_layout.locate(pos);
        self.segments[seg]
            .num_at_pos(local_pos)
            .map(|local_struct| local_struct + self.struct_layout.offset(seg))
    }
}
