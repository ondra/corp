use memmap::MmapOptions;
use fs_err::File;

use crate::text::as_slice_ref;
use crate::text::DeltaIter;
use crate::bits;

#[derive(Debug)]
pub struct Delta {
    crevf: memmap::Mmap,
    crdxf: memmap::Mmap,
    cntf: memmap::Mmap,
    alignmult: usize,
}

struct DeltaDense {

}

pub struct RevIter {


}

impl Iterator for RevIter {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {

        None
    }
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
        
        let maxid_val = rev.cntf.len() / 4;
        rev.alignmult = if as_slice_ref::<u32>(&rev.crdxf)[0] > 0 {
            (bits::Reader::open(as_slice_ref(&rev.crevf), 6*8).delta() - 1) as usize
        } else { 1 };
        
        Ok(rev)
    }

    pub fn count(&self, id: u32) -> usize {
        as_slice_ref::<u32>(&self.cntf)[id as usize] as usize
    }

    pub fn id2poss(&self, id: u32) -> DeltaIter {
        let maxid_val = self.cntf.len() / 4;
        if id > maxid_val as u32 {
            panic!();
        }
        let cnt = self.count(id);
        let seek = as_slice_ref::<u32>(&self.crdxf)[id as usize] as usize
            * self.alignmult;
        let mut rb = bits::Reader::open(as_slice_ref(&self.crevf), seek*8);
        // while rest != 0 { rb.delta(); rest -= 1; };
        DeltaIter { remaining: cnt as u64, rb }
    }
}
