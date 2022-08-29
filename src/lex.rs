use std::io::*;
use fs_err::File;
use std::str;
use std::cmp::Ordering;

use memmap::MmapOptions;

#[inline]
pub fn read<T: Sized>(mmap: &memmap::Mmap, idx: usize) -> T {
    let x = mmap.as_ptr() as *const T;
    unsafe { x.offset(idx as isize).read() }
}

#[derive(Debug)]
pub struct MapLex {
    pub name: String,
    lex: memmap::Mmap,
    srt: memmap::Mmap,
    idx: memmap::Mmap,
}

impl MapLex {
    pub fn open(base: &str) -> Result<MapLex> {
        let open_map = |name| {
            let f = File::open(base.to_string() + name)?;
            unsafe { MmapOptions::new().map(f.file()) }
        };

        Ok(MapLex{
            name: base.to_string(),
            lex: open_map(".lex")?,
            srt: open_map(".lex.srt")?,
            idx: open_map(".lex.idx")?,
        })
    }

    pub fn id2str(&self, id: u32) -> &str {
        let l: u32 = read(&self.idx, id as usize);
        let mut r: u32 = l;
        while r < self.lex.len() as u32 {
            if self.lex[r as usize] == 0 { break; }
            else { r = r + 1; }
        }
        return unsafe {
            std::str::from_utf8_unchecked(&self.lex[l as usize..r as usize])
        }
    }

    pub fn str2id(&self, s: &str) -> Option<u32> {
        let mut bot = 0 as u32;
        let mut top = (self.srt.len() / 4) as u32 - 1;
        while bot <= top {
            let cur_id = (top + bot) / 2;
            let sort_id: u32 = read(&self.srt, cur_id as usize);
            let q = self.id2str(sort_id);
            match q.cmp(s) {
                Ordering::Less => bot = cur_id + 1,
                Ordering::Greater => top = cur_id - 1,
                Ordering::Equal => return Some(sort_id),
            }
        }
        None
    }

    pub fn id_range(&self) -> u32 {
        (self.srt.len() / 4) as u32
    }
}
