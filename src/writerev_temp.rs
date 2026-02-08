use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::wrbits::BitsWriter;
use crate::writerev_sparse::REV_MAGIC;

fn add_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

pub struct TempRevWriter {
    bw: BitsWriter,
    idxf: BufWriter<File>,
    cntf: BufWriter<File>,
    alignmult: usize,
    next_id: u32,
    current_id: Option<u32>,
    current_count: u32,
    last_pos: Option<u64>,
}

impl TempRevWriter {
    pub fn create(base: &Path, alignmult: usize) -> Result<TempRevWriter, Box<dyn std::error::Error>> {
        if alignmult == 0 {
            return Err("alignmult must be >= 1".into());
        }

        let mut revf = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
        revf.write_all(&REV_MAGIC)?;
        let mut bw = BitsWriter::new(revf);
        bw.delta((alignmult as u64) + 1);

        let idxf = BufWriter::new(File::create(add_suffix(base, ".rev.idx"))?);
        let cntf = BufWriter::new(File::create(add_suffix(base, ".rev.cnt"))?);
        let _cntf64 = File::create(add_suffix(base, ".rev.cnt64"))?;

        Ok(TempRevWriter {
            bw,
            idxf,
            cntf,
            alignmult,
            next_id: 0,
            current_id: None,
            current_count: 0,
            last_pos: None,
        })
    }

    fn align_and_idx_value(&mut self) -> Result<u32, Box<dyn std::error::Error>> {
        self.bw.byte_align();
        if self.alignmult > 1 {
            let abs_bytes = (REV_MAGIC.len() as u64) + (self.bw.bits_written() / 8);
            let rem = (abs_bytes as usize) % self.alignmult;
            if rem != 0 {
                let pad_bytes = self.alignmult - rem;
                for _ in 0..pad_bytes {
                    for _ in 0..8 {
                        self.bw.bit(false);
                    }
                }
                self.bw.byte_align();
            }
        }

        let abs_bytes = (REV_MAGIC.len() as u64) + (self.bw.bits_written() / 8);
        if (abs_bytes as usize) % self.alignmult != 0 {
            return Err("temp rev alignment invariant broken".into());
        }
        let idx_val = abs_bytes / (self.alignmult as u64);
        if idx_val > u32::MAX as u64 {
            return Err("temp rev idx overflow".into());
        }
        Ok(idx_val as u32)
    }

    fn write_empty_id(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let idx = self.align_and_idx_value()?;
        self.idxf.write_all(&idx.to_le_bytes())?;
        self.cntf.write_all(&0u32.to_le_bytes())?;
        self.next_id = self.next_id.checked_add(1).ok_or("id overflow")?;
        Ok(())
    }

    fn finish_current_id(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.current_id.is_some() {
            self.cntf.write_all(&self.current_count.to_le_bytes())?;
            self.current_id = None;
            self.current_count = 0;
            self.last_pos = None;
        }
        Ok(())
    }

    fn start_id(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        while self.next_id < id {
            self.write_empty_id()?;
        }
        let idx = self.align_and_idx_value()?;
        self.idxf.write_all(&idx.to_le_bytes())?;
        self.current_id = Some(id);
        self.current_count = 0;
        self.last_pos = None;
        self.next_id = id.checked_add(1).ok_or("id overflow")?;
        Ok(())
    }

    pub fn put(&mut self, id: u32, pos: u64) -> Result<(), Box<dyn std::error::Error>> {
        match self.current_id {
            None => self.start_id(id)?,
            Some(cur) if id < cur => return Err("ids must be nondecreasing".into()),
            Some(cur) if id > cur => {
                self.finish_current_id()?;
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
            return Err("invalid zero gap in rev".into());
        }
        self.bw.delta(gap);
        self.last_pos = Some(pos);
        self.current_count = self.current_count.checked_add(1).ok_or("rev count overflow")?;
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

    pub fn fill_to(&mut self, max_id: u32) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(cur) = self.current_id {
            if cur > max_id {
                return Err("max_id smaller than current id".into());
            }
        }
        while self.next_id <= max_id {
            self.finish_current_id()?;
            self.write_empty_id()?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.finish_current_id()?;
        let mut revf = self.bw.finish()?;
        revf.flush()?;
        self.idxf.flush()?;
        self.cntf.flush()?;
        Ok(())
    }
}
