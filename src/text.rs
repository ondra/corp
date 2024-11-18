use fs_err::File;

use memmap::MmapOptions;

use crate::bits;
use crate::util::as_slice_ref;

#[derive(Debug)]
pub struct Delta {
    pub name: String,
    text: memmap::Mmap,
    seg: memmap::Mmap,
    positions: usize,
    segment_size: usize,
}

#[derive(Debug)]
pub struct BigDelta {

}

#[derive(Debug)]
pub struct GigaDelta {
    pub name: String,
    text: memmap::Mmap,
    offset: memmap::Mmap,
    segment: memmap::Mmap,
    positions: usize,
}

pub trait Text: std::fmt::Debug {
    fn posat(&self, pos: u64) -> Option<DeltaIter<'_>>;
    fn structat(&self, pos: u64) -> Option<IntIter<'_>>;
    fn size(&self) -> usize;
    fn get(&self, pos: u64) -> u32;
}

impl GigaDelta {
    pub fn open(base: &str) -> Result<GigaDelta, std::io::Error> {
        let text = File::open(base.to_string() + ".text")?;
        let seg = File::open(base.to_string() + ".text.seg")?;
        let offset = File::open(base.to_string() + ".text.off")?;

        let mut gdt = GigaDelta {
            positions: 0,
            name: base.to_string(),
            text: unsafe { MmapOptions::new().map(text.file())? },
            segment: unsafe { MmapOptions::new().map(seg.file())? },
            offset: unsafe { MmapOptions::new().map(offset.file())? },
        };

        let mut rb = bits::Reader::open(as_slice_ref(&gdt.text), 16*8);

        let _segment_size = rb.delta() - 1;
        gdt.positions = (rb.delta() - 1) as usize;

        Ok(gdt)
    }

    pub fn at(&self, pos: u64) -> DeltaIter {
        let mut rest = pos % 64;
        let seek = (as_slice_ref::<u16>(&self.offset))[pos as usize/64] as usize;
        let seek = seek + 
            ((as_slice_ref::<u32>(&self.segment))[pos as usize/(64*16)]) as usize * 2048*8;
        let mut rb = bits::Reader::open(as_slice_ref(&self.text), seek as usize);
        while rest != 0 { rb.delta(); rest -= 1; };
        DeltaIter { remaining: self.positions as u64 - pos, rb }
    }

    pub fn size(&self) -> usize {
        self.positions
    }
}

impl Text for Delta {
    fn posat(&self, pos: u64) -> Option<DeltaIter> { Some(self.at(pos)) }
    fn structat(&self, _pos: u64) -> Option<IntIter<'_>> { None }
    fn size(&self) -> usize { self.size() }
    fn get(&self, pos: u64) -> u32 { self.at(pos).next().unwrap() }
}

#[derive(Debug)]
pub struct DeltaIter<'a> {
    pub remaining: u64,
    pub rb: bits::Reader<'a>
}

impl Iterator for DeltaIter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.remaining > 0 {
            self.remaining -= 1;
            Some(self.rb.delta() as u32 - 1)
        } else { None }
    }
}

impl Delta {
    pub fn open(base: &str) -> Result<Delta, std::io::Error> {
        let text = File::open(base.to_string() + ".text")?;
        let seg = File::open(base.to_string() + ".text.seg")?;

        let mut dt = Delta {
            positions: 0,
            segment_size: 0,
            name: base.to_string(),
            text: unsafe { MmapOptions::new().map(text.file())? },
            seg: unsafe { MmapOptions::new().map(seg.file())? },
        };

        let mut rb = bits::Reader::open(as_slice_ref(&dt.text), 16*8);
        dt.segment_size = (rb.delta() - 1) as usize;
        dt.positions = (rb.delta() - 1) as usize;
        Ok(dt)
    }

    pub fn at(&self, pos: u64) -> DeltaIter {
        let segslice = as_slice_ref::<u32>(&self.seg);
        let sp = segslice[pos as usize / self.segment_size];
        let mut rest = pos % self.segment_size as u64;
        let mut rb = bits::Reader::open(as_slice_ref(&self.text), sp as usize);
        while rest != 0 { rb.delta(); rest -= 1; };
        DeltaIter { remaining: self.positions as u64 - pos, rb }
    }

    pub fn size(&self) -> usize {
        self.positions
    }
}

impl Text for GigaDelta {
    fn posat(&self, pos: u64) -> Option<DeltaIter> { Some(self.at(pos)) }
    fn structat(&self, _pos: u64) -> Option<IntIter<'_>> { None }
    fn size(&self) -> usize { self.size() }
    fn get(&self, pos: u64) -> u32 { self.at(pos).next().unwrap() }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Int {
    name: String,
    positions: usize,
    text: memmap::Mmap,
}

impl Int {
    pub fn open(base: &str) -> Result<Int, std::io::Error> {
        let text = File::open(base.to_string() + ".text")?;
        let mut t = Int {
            positions: 0,
            name: base.to_string(),
            text: unsafe { MmapOptions::new().map(text.file())? },
        };

        t.positions = (t.text.len() / 4) as usize - 4;
        Ok(t)
    }

    pub fn get(&self, pos: u64) -> u32 {
        as_slice_ref::<u32>(&self.text)[pos as usize + 4]
    }

    pub fn size(&self) -> usize {
        self.positions
    }
}

#[derive(Debug)]
pub struct IntIter<'a> {
    pub position: usize,
    pub inttext: &'a Int,
}

impl Iterator for IntIter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.position < self.inttext.positions {
            let ret = Some(self.inttext.get(self.position as u64));
            self.position += 1;
            ret
        } else { None }
    }
}

impl Text for Int {
    fn structat(&self, pos: u64) -> Option<IntIter<'_>> {
        Some(IntIter {
            position: pos as usize,
            inttext: &self,
        })
    }
    fn posat(&self, _pos: u64) -> Option<DeltaIter<'_>> { None }
    fn size(&self) -> usize { self.positions as usize }
    fn get(&self, pos: u64) -> u32 { self.get(pos) }
}
