use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::wrbits::BitsWriter;

pub const REV_DENSE_MAGIC: [u8; 6] = [0xa8, b'f', b'i', b'n', b'D', b'R'];

fn add_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

pub struct DenseRevWriter {
    base: PathBuf,
    bw: BitsWriter,
    byte_offsets: Vec<u32>,
    counts: Vec<u32>,
    next_id: u32,
    current_id: Option<u32>,
    current_idx: Option<usize>,
    last_pos: Option<u64>,
}

impl DenseRevWriter {
    pub fn create(base: &Path) -> Result<DenseRevWriter, Box<dyn std::error::Error>> {
        let mut revf = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
        revf.write_all(&REV_DENSE_MAGIC)?;
        let bw = BitsWriter::new(revf);
        Ok(DenseRevWriter {
            base: base.to_path_buf(),
            bw,
            byte_offsets: Vec::new(),
            counts: Vec::new(),
            next_id: 0,
            current_id: None,
            current_idx: None,
            last_pos: None,
        })
    }

    fn current_byte_offset(&self) -> u64 {
        (REV_DENSE_MAGIC.len() as u64) + (self.bw.bits_written() / 8)
    }

    fn write_empty_id(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.bw.byte_align();
        let off = self.current_byte_offset();
        if off > u32::MAX as u64 {
            return Err("rev dense offset overflow".into());
        }
        self.byte_offsets.push(off as u32);
        self.counts.push(0);
        self.next_id = self.next_id.checked_add(1).ok_or("id overflow")?;
        Ok(())
    }

    fn finish_current_id(&mut self) {
        self.current_id = None;
        self.current_idx = None;
        self.last_pos = None;
    }

    fn start_id(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        while self.next_id < id {
            self.write_empty_id()?;
        }
        self.bw.byte_align();
        let off = self.current_byte_offset();
        if off > u32::MAX as u64 {
            return Err("rev dense offset overflow".into());
        }
        self.byte_offsets.push(off as u32);
        self.counts.push(0);
        self.current_id = Some(id);
        self.current_idx = Some(self.counts.len() - 1);
        self.last_pos = None;
        self.next_id = id.checked_add(1).ok_or("id overflow")?;
        Ok(())
    }

    pub fn put(&mut self, id: u32, pos: u64) -> Result<(), Box<dyn std::error::Error>> {
        match self.current_id {
            None => self.start_id(id)?,
            Some(cur) if id < cur => return Err("ids must be nondecreasing".into()),
            Some(cur) if id > cur => {
                self.finish_current_id();
                self.start_id(id)?;
            }
            _ => {}
        }

        let gap = match self.last_pos {
            None => pos.checked_add(1).ok_or("rev position overflow")?,
            Some(prev) if pos > prev => pos - prev,
            Some(_) => return Err("positions for same id must be strictly increasing".into()),
        };
        if gap == 0 {
            return Err("invalid zero gap in rev dense".into());
        }
        self.bw.delta(gap);
        self.last_pos = Some(pos);
        let idx = self.current_idx.ok_or("dense writer internal state error")?;
        self.counts[idx] = self.counts[idx]
            .checked_add(1)
            .ok_or("rev count overflow")?;
        Ok(())
    }

    pub fn put_batch<I>(&mut self, id: u32, positions: I) -> Result<(), Box<dyn std::error::Error>>
    where
        I: IntoIterator<Item = u64>,
    {
        for pos in positions {
            self.put(id, pos)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.finish_current_id();
        let mut revf = self.bw.finish()?;
        revf.flush()?;

        if self.byte_offsets.is_empty() {
            return Err("dense rev writer received no ids".into());
        }

        let idx1 = BufWriter::new(File::create(add_suffix(&self.base, ".rev.idx1"))?);
        let mut bw1 = BitsWriter::new(idx1);
        let mut idx0: Vec<u32> = Vec::new();
        let mut block_start = 0usize;
        while block_start < self.byte_offsets.len() {
            bw1.byte_align();
            let idx1_byte = bw1.bits_written() / 8;
            if idx1_byte > u32::MAX as u64 {
                return Err("rev dense idx1 overflow".into());
            }
            idx0.push(idx1_byte as u32);

            let mut last_off: u32 = 0;
            let end = std::cmp::min(block_start + 64, self.byte_offsets.len());
            for i in block_start..end {
                let off = self.byte_offsets[i];
                let delta = off.wrapping_sub(last_off);
                if delta == 0 {
                    return Err("invalid zero delta in rev dense".into());
                }
                bw1.delta(delta as u64);
                bw1.gamma(self.counts[i] as u64 + 1);
                last_off = off;
            }
            bw1.delta(1);
            bw1.gamma(1);
            block_start += 64;
        }
        let mut idx1f = bw1.finish()?;
        idx1f.flush()?;
        let idx1_end = idx1f.seek(SeekFrom::Current(0))?;
        if idx1_end > u32::MAX as u64 {
            return Err("rev dense idx1 overflow".into());
        }
        idx0.push(idx1_end as u32);
        idx0.push(self.byte_offsets.len() as u32);

        let mut idx0f = BufWriter::new(File::create(add_suffix(&self.base, ".rev.idx0"))?);
        for off in idx0 {
            idx0f.write_all(&off.to_le_bytes())?;
        }
        idx0f.flush()?;
        Ok(())
    }
}
