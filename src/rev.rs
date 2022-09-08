use memmap::MmapOptions;
use fs_err::File;
use std::fmt;

use crate::text::DeltaIter;
use crate::bits;
use crate::util::as_slice_ref;

#[derive(Debug)]
struct BadRevHeader {
    path: String
}

#[derive(Debug)]
pub struct RevIter<'a> {
    di: DeltaIter<'a>,
    last: i64,
}

impl Iterator for RevIter<'_> {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        if let Some(v) = self.di.next() {
            self.last += (v + 1) as i64; 
            Some(self.last as u64)
        } else { None }
    }
}

impl fmt::Display for BadRevHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BadRevHeader({})", self.path)
    }
}

impl std::error::Error for BadRevHeader {}

pub fn open(base: &str) -> Result<Box<dyn Rev + Sync + Send>, Box<dyn std::error::Error>> {
    let revf = File::open(base.to_string() + ".rev")?;
    let rev = unsafe { MmapOptions::new().map(revf.file())? };

    match &rev[0..6] {
        b"\xa3finDR" => Ok(Box::new(Delta::open(base)?)),
        b"\xa8finDR" => Ok(Box::new(DeltaDense::open(base)?)),
        _ => Err(Box::new(BadRevHeader{path: base.to_string()})),
    }
}

pub trait Rev: std::fmt::Debug {
    fn count(&self, id: u32) -> u64;
    fn id2poss(&self, id: u32) -> RevIter;
}

impl Rev for Delta {
    fn count(&self, id: u32) -> u64 { self.count(id) }
    fn id2poss(&self, id: u32) -> RevIter { self.id2poss(id) }
}

impl Rev for DeltaDense {
    fn count(&self, id: u32) -> u64 { self.count(id) }
    fn id2poss(&self, id: u32) -> RevIter { self.id2poss(id) }
}

#[derive(Debug)]
pub struct Delta {
    crevf: memmap::Mmap,
    crdxf: memmap::Mmap,
    cntf: memmap::Mmap,
    alignmult: usize,
}

#[derive(Debug)]
pub struct DeltaDense {
    crevf: memmap::Mmap,
    crdxf0: memmap::Mmap,
    crdxf1: memmap::Mmap,
}


impl Delta {
    pub fn open(base: &str) -> Result<Delta, Box<dyn std::error::Error>> {
        let crevf = File::open(base.to_string() + ".rev")?;
        let crdxf = File::open(base.to_string() + ".rev.idx")?;
        let cntf = File::open(base.to_string() + ".rev.cnt")?;

        let mut rev = unsafe {Delta {
            crevf: MmapOptions::new().map(crevf.file())?,
            crdxf: MmapOptions::new().map(crdxf.file())?,
            cntf: MmapOptions::new().map(cntf.file())?,
            alignmult: 0,
        }};
        
        let _maxid_val = rev.cntf.len() / 4;
        rev.alignmult = if as_slice_ref::<u32>(&rev.crdxf)[0] > 0 {
            (bits::Reader::open(as_slice_ref(&rev.crevf), 6*8).delta() - 1) as usize
        } else { 1 };
        
        Ok(rev)
    }

    pub fn count(&self, id: u32) -> u64 {
        as_slice_ref::<u32>(&self.cntf)[id as usize] as u64
    }

    pub fn id2poss(&self, id: u32) -> RevIter {
        let maxid_val = self.cntf.len() / 4;
        if id > maxid_val as u32 {
            panic!();
        }
        let cnt = self.count(id);
        let seek = as_slice_ref::<u32>(&self.crdxf)[id as usize] as usize
            * self.alignmult;
        let rb = bits::Reader::open(as_slice_ref(&self.crevf), seek*8);
        // while rest != 0 { rb.delta(); rest -= 1; };
        //DeltaIter { remaining: cnt as u64, rb }
        RevIter { di: DeltaIter { remaining: cnt as u64, rb }, last: -1 }
    }
}

impl DeltaDense {
    pub fn open(base: &str) -> Result<DeltaDense, Box<dyn std::error::Error>> {
        let crevf = File::open(base.to_string() + ".rev")?;
        let crdxf0 = File::open(base.to_string() + ".rev.idx0")?;
        let crdxf1 = File::open(base.to_string() + ".rev.idx1")?;

        let rev = unsafe {DeltaDense {
            crevf: MmapOptions::new().map(crevf.file())?,
            crdxf0: MmapOptions::new().map(crdxf0.file())?,
            crdxf1: MmapOptions::new().map(crdxf1.file())?,
        }};

        let _maxid_val = as_slice_ref::<u32>(&rev.crdxf0)[0];

        Ok(rev)
    }

    fn locate(&self, id: u32) -> (usize, u64) {
        let block_seek = as_slice_ref::<u32>(&self.crdxf0)
            [id as usize / 64] as usize;
        let rem = id % 64;

        let mut rb = bits::Reader::open(as_slice_ref(&self.crdxf1), block_seek*8);
        let mut seek = 0 as usize;
        let mut cnt = 0 as u64;

        for _blkpos in 0 ..= rem {
            seek += rb.delta() as usize;
            cnt = rb.gamma() - 1;
        }
        (seek, cnt)
    }

    pub fn count(&self, id: u32) -> u64 {
        let (_, cnt) = self.locate(id);
        cnt as u64
    }

    pub fn id2poss(&self, id: u32) -> RevIter {
        let (seek, cnt) = self.locate(id);
        let rb = bits::Reader::open(as_slice_ref(&self.crevf), seek as usize*8);
        RevIter {di: DeltaIter { remaining: cnt, rb }, last: -1 }
    }
}
