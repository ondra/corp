use std::io::*;
use fs_err::File;
use std::str;
//use std::cmp::Ordering;

use memmap::MmapOptions;

#[inline]
pub fn read<T: Sized>(mmap: &memmap::Mmap, idx: usize) -> T {
    let x = mmap.as_ptr() as *const T;
    unsafe { x.offset(idx as isize).read() }
}

#[derive(Debug)]
pub struct MapStructure32 {
    name: String,
    rng: memmap::Mmap,
}

#[derive(Debug)]
pub struct MapStructure64 {
    name: String,
    rng: memmap::Mmap,
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
    
    pub fn beg_at(&self, pos: u32) -> u64 {
        read(&self.rng, (pos * 2) as usize)
    }
    
    pub fn end_at(&self, pos: u32) -> u64 {
        read(&self.rng, (pos * 2 + 1) as usize)
    }
}
