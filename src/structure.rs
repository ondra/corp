use std::io::*;
use fs_err::File;
use std::str;
//use std::cmp::Ordering;

use memmap::MmapOptions;
use crate::util::as_slice_ref;

#[inline]
pub fn read<T: Sized>(mmap: &memmap::Mmap, idx: usize) -> T {
    let x = mmap.as_ptr() as *const T;
    unsafe { x.add(idx).read() }
}

#[derive(Debug)]
pub struct MapStructure32 {
    pub name: String,
    rng: memmap::Mmap,
}

#[derive(Debug)]
pub struct MapStructure64 {
    pub name: String,
    rng: memmap::Mmap,
}

impl MapStructure64 {
    pub fn open(base: &str) -> Result<MapStructure64> {
        let open_map = |name| {
            let f = File::open(base.to_string() + name)?;
            unsafe { MmapOptions::new().map(f.file()) }
        };

        Ok(MapStructure64{
            name: base.to_string(),
            rng: open_map(".rng")?,
        })
    }
    
    pub fn beg_at(&self, pos: u64) -> u64 {
        read(&self.rng, (pos * 2) as usize)
    }
    
    pub fn end_at(&self, pos: u64) -> u64 {
        as_slice_ref::<u64>(&self.rng)[(pos * 2 + 1) as usize] as u64
    }
}

impl MapStructure32 {
    pub fn open(base: &str) -> Result<MapStructure32> {
        let open_map = |name| {
            let f = File::open(base.to_string() + name)?;
            unsafe { MmapOptions::new().map(f.file()) }
        };
        Ok(MapStructure32{
            name: base.to_string(),
            rng: open_map(".rng")?,
        })
    }
    pub fn beg_at(&self, pos: u64) -> u64 {
        read::<u32>(&self.rng, (pos * 2) as usize) as u64
    }
    pub fn end_at(&self, pos: u64) -> u64 {
        read::<u32>(&self.rng, (pos * 2 + 1) as usize) as u64
    }
}

pub fn open(base: &str, type64: bool) -> std::result::Result<Box<dyn Struct + Sync + Send>,
    Box<dyn std::error::Error>> {
    Ok(if type64 { Box::new(MapStructure64::open(base)?) }
    else { Box::new(MapStructure32::open(base)?) })
}

pub trait Struct: std::fmt::Debug {
    fn beg_at(&self, pos: u64) -> u64;
    fn end_at(&self, pos: u64) -> u64;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 }
    /*
    fn find_beg(&self, pos: u64, start_at_struct_pos: u64) -> Option<u64> {
        let mut incr = 1u64;
        let mut curr = 0u64;
        let len = self.len() as u64;

        while (curr + incr) < len && self.beg_at(curr + incr) <= pos {
            curr += incr;
            incr *= 2;
        }
        while incr > 0 {
            if (curr + incr) < len && self.beg_at(curr + incr) <= pos {
                curr += incr;
            }
            incr /= 2;
        }
        //if self.beg_at(curr) < pos {
        //    curr += 1;
        //}
        if pos >= self.beg_at(curr) && pos < self.end_at(curr) {
            Some(curr)
        } else {
            None
        }
    }
    */
    fn find_end(&self, pos: u64) -> (u64, u64) {
        find_end_i(self, pos)
    }
    fn find_beg(&self, pos: u64) -> u64 {
        find_beg_i(self, pos)
    }
    fn num_at_pos(&self, pos: u64) -> Option<u64> {
        num_at_pos_i(self, pos)
    }
}


impl Struct for MapStructure32 {
    fn beg_at(&self, pos: u64) -> u64 { self.beg_at(pos) }
    fn end_at(&self, pos: u64) -> u64 { self.end_at(pos) }
    fn len(&self) -> usize { self.rng.len() / 8 }
}

impl Struct for MapStructure64 {
    fn beg_at(&self, pos: u64) -> u64 { self.beg_at(pos) }
    fn end_at(&self, pos: u64) -> u64 { self.end_at(pos) }
    fn len(&self) -> usize { self.rng.len() / 16 }
}

fn find_end_i(s: &(impl Struct + ?Sized), pos: u64) -> (u64, u64) {
    let mut curr = 0u64;
    let last = s.len() as u64 - 1;
    let finval = u64::MAX;
    if !(curr < last) { return (finval, finval); }
    let mut incr = 1u64;
    while (curr + incr) < last &&
        /* abs( */ s.end_at(curr + incr) /* ) */ <= pos
            {
        curr += incr;
        incr *= 2;
    }
    while incr > 0 {
        if (curr + incr) < last &&
            /* abs( */ s.end_at(curr + incr) /* ) */ <= pos
                {
            curr += incr;
        }
        incr /= 2;
    }
    /*
    // go back out of nested ranges (end < 0)
    while (prev < curr && s.end_at(curr) < 0) {
        curr -= 1;
    }
    */
    while curr < last && s.end_at(curr) /*.abs()*/ < pos {
        curr += 1;
    }
    if curr < last { (curr, s.beg_at(curr)) }
    else { (finval, finval) }
}

fn find_beg_i(s: &(impl Struct + ?Sized), pos: u64) -> u64 {
    let mut curr = 0u64;
    let prev = curr;
    let last = s.len() as u64 - 1;
    let finval = u64::MAX;
    if !(curr < last) { return finval; }
    let mut incr = 1u64;
    while (curr + incr) < last &&
        /* abs( */ s.beg_at(curr + incr) /* ) */ <= pos {
        curr += incr;
        incr *= 2;
    }
    while incr > 0 {
        if (curr + incr) < last &&
            /* abs( */ s.beg_at(curr + incr) /* ) */ <= pos {
            curr += incr;
        }
        incr /= 2;
    }
    if s.beg_at(curr) < pos {
        curr += 1;
    } else if prev < curr {
        let mut curr_1 = curr;
        curr_1 -= 1;
        while prev < curr && s.beg_at(curr_1) == pos {
            curr -= 1;
            curr_1 -= 1;
        }
    }
    if curr < last { s.beg_at(curr) }
    else { finval }
}

fn num_at_pos_i(s: &(impl Struct + ?Sized), pos: u64) -> Option<u64> {
    let (mut structpos, corppos) = s.find_end(pos + 1);
    if structpos == u64::MAX {
        return None;
    }
    if corppos <= pos {
        // let rngsize = s.end_at(b) - s.beg_at(b);

        // try to find smaller range in nested ones
        // TODO
        return Some(structpos);
    }
    // handling of empty structures around pos
    if s.end_at(structpos) == s.beg_at(structpos) && s.beg_at(structpos) == pos + 1 {
        // empty struct following pos
        return Some(structpos);
    }
    if structpos > 0 {
        structpos -= 1;
    }
    if s.beg_at(structpos) == s.beg_at(structpos) && s.beg_at(structpos) == pos {
        return Some(structpos)
    }
    return None
}
