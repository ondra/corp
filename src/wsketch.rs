// use fs_err::File;

use crate::bits;
use crate::util::as_slice_ref;

use crate::lex::MapLex;
use crate::corp::Attr;

use std::fmt;

#[derive(Debug)]
pub struct WMap {
    pub name: String,

    levels: [memmap::Mmap; 3],
    _levelsizes: [usize; 3],

    final_id1: u32,
    map0idx: memmap::Mmap,
    has_commonest: bool,
    has_ftt: bool,
    adjust_idx: bool,

    min_sc: f32,
    norm_sc: f32,

    pub version: u32,
    rev: WMapRev,
}

impl WMap {
    pub fn new(base: &str) -> Result<WMap, Box<dyn std::error::Error>> {
        let revf = fs_err::File::open(base.to_string() + ".rev")?;
        let map0idxf = fs_err::File::open(base.to_string() + ".map0.idx")?;

        let open_level = |levelno| -> std::result::Result<memmap::Mmap, Box<dyn std::error::Error>> {
            let lf = fs_err::File::open(
                format!("{}.map{}.com", base, levelno))?;
            Ok(unsafe { memmap::MmapOptions::new().map(lf.file())? })
        };
        let levels = [open_level(0)?, open_level(1)?, open_level(2)?];
        let mut _levelsizes = [0usize, 0, 0];
        let mut version = 4u32;
        let mut final_id1 = 0u32;

        let mut has_commonest = false;
        let mut has_ftt = false;
        let mut adjust_idx = false;
        let mut min_sc = -10f32;
        let mut norm_sc = (1<<12) as f32 / 30f32;

        for i in 0..3 {
            let mut rb = bits::Reader::open(as_slice_ref(&levels[i]), 16*8);
            _levelsizes[i] = rb.delta() as usize;

            match i {
                0 => {
                    version += levels[i][10] as u32;
                    final_id1 = rb.delta() as u32;
                    if version > 4 {
                        final_id1 = final_id1.wrapping_sub(1u32);
                    }
                },
                1 => {},
                2 => {
                    has_commonest = rb.bit();
                    adjust_idx = rb.bit();
                    if adjust_idx {
                        let sc_bits = rb.delta();
                        let max_sc = rb.delta() as f32;
                        min_sc = -((rb.delta() -1) as f32);
                        norm_sc = (1 << sc_bits) as f32 / (max_sc - min_sc);
                    }
                    if version > 5 {
                        has_ftt = rb.bit();
                    }
                },
                _ => panic!(),
            }
        }
        let revm = unsafe { memmap::MmapOptions::new().map(revf.file())? };
        Ok(WMap {
            name: base.to_string(),
            map0idx: unsafe { memmap::MmapOptions::new().map(map0idxf.file())? },
            rev: WMapRev::new(revm),
            levels, _levelsizes, final_id1, has_commonest,
            has_ftt, adjust_idx, min_sc, norm_sc, version
        })
    }

    fn get_block_seek(&self, id: u32) -> u32 {
        let mut block: usize = id as usize / 64;
        let map0idxm: &[u32] = as_slice_ref(&self.map0idx);
        if block >= map0idxm.len() {
            block = map0idxm.len() - 1;
        }
        map0idxm[block]
    }

    fn iter_from(&self, pos: u32) -> WMapIter1 {
        let max_block_items = 64;
        let mut it = WMapIter1 {
            wmap: self,
            rb: bits::Reader::open(
                as_slice_ref(&self.levels[0]), pos as usize),
            idx: 0, id: 0
        };
        for _ in 0..(pos % max_block_items) {
            it.next();
        }
        it
    }

    pub fn find_id(&self, id: u32) -> Option<WMapItem1<'_>> {
        let bs = self.get_block_seek(id);
        for v in self.iter_from(bs) {
            if v.id < id { continue; }
            else if v.id == id { return Some(v); }
            else { return None; }            
        }
        None 
    }

    pub fn iter_ids(&self) -> WMapIter1 {
        let it = WMapIter1{
            wmap: self,
            rb: bits::Reader::open(as_slice_ref(&self.levels[0]), 32*8),
            idx: 0, id: 0
        };
        it
    }
}

fn read_record(rb: &mut bits::Reader<'_>,
               idx: &mut usize, id: &mut u32, adjust_idx: bool) {
    let add = rb.delta();
    if add > 1 {
        *idx += add as usize;
        if adjust_idx {
            *idx -= 1;
        }
        // items_from_sync += 1;
        *id += rb.delta() as u32;
    } else {
        // last_sync = rb.tell();
        // items_from_sync = 0;
        *idx = rb.delta() as usize;
        *id = rb.delta() as u32 - 1;
    }
}


// Level 1

#[derive(Debug)]
pub struct WMapItem1<'a> { wmap: &'a WMap,
    pub id: u32, pub idx: usize, pub cnt: u64, pub frq: u64 }
impl fmt::Display for WMapItem1<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(1 id:{} seek:{} cnt:{} frq:{})", self.id, self.idx, self.cnt, self.frq)
    }
}

pub struct WMapIter1<'a> { wmap: &'a WMap,
    rb: bits::Reader<'a>,
    id: u32, idx: usize }

impl <'a> Iterator for WMapIter1<'a> {
    type Item = WMapItem1<'a>;
    fn next(&mut self) -> Option<WMapItem1<'a>> {
        if self.id >= self.wmap.final_id1 { return None }
        let adjust_idx = false;
        read_record(&mut self.rb, &mut self.idx, &mut self.id, adjust_idx);
        let cnt = self.rb.delta() as u64;
        let frq = self.rb.delta() as u64;
        Some(WMapItem1 { wmap: self.wmap,
            id: self.id, idx: self.idx, cnt, frq })
    }
}

impl WMapItem1<'_> {
    pub fn iter(&self) -> WMapIter2<'_> {
        WMapIter2 {
            wmap: self.wmap, 
            rb: bits::Reader::open(as_slice_ref(&self.wmap.levels[1]), self.idx),
            remaining: self.cnt as usize,
            id: 0, idx: 0
        }
    }
}

// Level 2

#[derive(Debug)]
pub struct WMapItem2<'a> { wmap: &'a WMap,
    pub id: u32, pub idx: usize, pub cnt: u64, pub frq: u64, pub rnk: f32 }
impl fmt::Display for WMapItem2<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(2 id:{} seek:{} cnt:{} frq:{} rnk:{})", self.id, self.idx, self.cnt, self.frq, self.rnk)
    }
}
pub struct WMapIter2<'a> {
    wmap: &'a WMap,
    rb: bits::Reader<'a>,
    id: u32, idx: usize,
    remaining: usize,
}

impl <'a> Iterator for WMapIter2<'a> {
    type Item = WMapItem2<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 { return None } else { self.remaining -= 1; }
        let adjust_idx = false;
        read_record(&mut self.rb, &mut self.idx, &mut self.id, adjust_idx);
        let cnt = self.rb.delta() as u64;
        let rnk = (self.rb.delta() as f32) 
            / self.wmap.norm_sc + self.wmap.min_sc;
        let frq = self.rb.delta() as u64;
        Some(WMapItem2 { wmap: self.wmap, 
            id: self.id, idx: self.idx, cnt, frq, rnk })
    }
}

impl WMapItem2<'_> {
    pub fn iter(&self) -> WMapIter3<'_> {
        WMapIter3 {
            wmap: self.wmap, 
            rb: bits::Reader::open(as_slice_ref(&self.wmap.levels[2]), self.idx),
            remaining: self.cnt as usize,
            id: 0, idx: 0
        }
    }
}


// Level 3

#[derive(Debug)]
pub struct WMapItem3<'a> { wmap: &'a WMap,
    pub id: u32, pub idx: usize, pub cnt: u64, pub frq: u64, pub rnk: f32 }
impl fmt::Display for WMapItem3<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(3 id:{} seek:{} cnt:{} frq:{} rnk:{})", self.id, self.idx, self.cnt, self.frq, self.rnk)
    }
}
pub struct WMapIter3<'a> {
    wmap: &'a WMap,
    rb: bits::Reader<'a>,
    id: u32, idx: usize,
    remaining: usize,
}

impl <'a> Iterator for WMapIter3<'a> {
    type Item = WMapItem3<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 { return None } else { self.remaining -= 1; }
        let adjust_idx = self.wmap.adjust_idx;
        read_record(&mut self.rb, &mut self.idx, &mut self.id, adjust_idx);
        let cnt = self.rb.delta() as u64;
        let rnk = (self.rb.delta() as f32) 
            / self.wmap.norm_sc + self.wmap.min_sc;
        let frq = self.rb.delta() as u64;
        if self.wmap.has_commonest {
            let len = self.rb.gamma();
            for _ in 0..len-1 {
                let _lcm = self.rb.delta() -1;
            }
        }
        if self.wmap.has_ftt {
            let len = self.rb.gamma();
            for _ in 0..len-1 {
                let _ftt = self.rb.delta() -1;
            }
        }
        Some(WMapItem3 { wmap: self.wmap,
            id: self.id, idx: self.idx, cnt, frq, rnk })
    }
}

impl WMapItem3<'_> {
    pub fn iter(&self) -> WMapRevStream<'_> {
        self.wmap.rev.poss(self.idx, self.cnt as usize)
    }
}

// Rev

#[derive(Debug)]
pub struct WMapRev {
    revm: memmap::Mmap,
    alignmult: usize,
    // corpsize: u64,
    adjust_pos: bool,
}

impl WMapRev {
    fn new(revm: memmap::Mmap) -> WMapRev {
        let mut rb = bits::Reader::open(as_slice_ref(&revm), 16*8);
        let mut alignmult = rb.delta();                
        let _corpsize = rb.delta();
        let adjust_pos = alignmult == 2;
        if adjust_pos { alignmult = 1; }
        WMapRev { revm, alignmult: alignmult as usize, adjust_pos }
    }
    fn poss(&self, from: usize, cnt: usize) -> WMapRevStream<'_> {
        let bitpos = 8 * from * self.alignmult;
        WMapRevStream {
            rb: bits::Reader::open(as_slice_ref(&self.revm), bitpos),
            remaining: cnt,
            adjust_pos: self.adjust_pos,
            curpos: 0,
        }
    }
}

pub struct WMapRevStream<'a> {
    rb: bits::Reader<'a>,
    remaining: usize,
    adjust_pos: bool, 
    curpos: i64,
}

impl Iterator for WMapRevStream<'_> {
    type Item = (usize, Option<i32>);
    fn next(&mut self) -> Option<(usize, Option<i32>)> {
        if self.remaining == 0 { return None; }
        self.remaining -= 1;

        self.curpos += self.rb.delta() as i64;
        if self.adjust_pos { self.curpos -= 1; }
 
        let mut c = self.rb.gamma() as i64;
        let coll = if c == 1 {
            None
        } else {
            if c % 2 == 1 { c = -c; }
            assert!(self.rb.gamma() == 1);
            Some((c / 2) as i32)
        };
        Some((self.curpos as usize, coll))
    }
}

impl ExactSizeIterator for WMapRevStream<'_> {
    fn len(&self) -> usize { self.remaining }
}

pub struct WSLex<'a> {
    grlex: MapLex,
    colllex: Option<MapLex>,
    wsattr: &'a dyn Attr,
}

impl WSLex<'_> {
    pub fn open<'a, 'b>(wsbase: &'a str, wsattr: &'b dyn Attr)
            -> Result<WSLex<'b>, Box<dyn std::error::Error>> {
        let grlex = MapLex::open(wsbase)?;
        let ml = MapLex::open(&(wsbase.to_string() + ".coll"));
        let colllex = match ml {  // distinguish between error and nonexistence
            Ok(a) => Some(a),
            Err(e@std::io::Error {..}) => {
                if e.kind() == std::io::ErrorKind::InvalidInput { None }
                else { return Err(Box::new(e)); }
            },
        };
        Ok(WSLex { grlex, colllex, wsattr })
    }

    pub fn coll2id(&self, coll: &str) -> u32 {
        if let Some(id) = self.wsattr.str2id(coll) {
            id
        } else {
            self.colllex.as_ref().unwrap().str2id(coll).unwrap() - self.wsattr.id_range()
        }
    }

    pub fn id2coll(&self, id: u32) -> &str {
        if id > self.wsattr.id_range() {
            self.colllex.as_ref().unwrap().id2str(id - self.wsattr.id_range())
        } else {
            self.wsattr.id2str(id)
        }
    }

    pub fn id2head(&self, id: u32) -> &str { self.wsattr.id2str(id) }
    pub fn head2id(&self, head: &str) -> Option<u32> { self.wsattr.str2id(head) }

    pub fn id2rel(&self, id: u32) -> &str { self.grlex.id2str(id) }
    pub fn rel2id(&self, head: &str) -> Option<u32> { self.grlex.str2id(head) }
}
