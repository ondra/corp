use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::wrbits::BitsWriter;

pub const REV_MAGIC: [u8; 6] = [0xa3, b'f', b'i', b'n', b'D', b'R'];

fn add_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

pub struct SparseRevWriter {
    bw: BitsWriter,
    idxf: BufWriter<File>,
    cntf: BufWriter<File>,
    next_id: u32,
    current_id: Option<u32>,
    current_count: u32,
    last_pos: Option<u64>,
}

impl SparseRevWriter {
    pub fn create(base: &Path) -> Result<SparseRevWriter, Box<dyn std::error::Error>> {
        let mut revf = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
        revf.write_all(&REV_MAGIC)?;
        let mut bw = BitsWriter::new(revf);
        bw.delta(2); // alignmult=1, stored as delta(alignmult+1)
        Ok(SparseRevWriter {
            bw,
            idxf: BufWriter::new(File::create(add_suffix(base, ".rev.idx"))?),
            cntf: BufWriter::new(File::create(add_suffix(base, ".rev.cnt"))?),
            next_id: 0,
            current_id: None,
            current_count: 0,
            last_pos: None,
        })
    }

    fn current_byte_offset(&self) -> u64 {
        (REV_MAGIC.len() as u64) + (self.bw.bits_written() / 8)
    }

    fn write_empty_id(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.bw.byte_align();
        let off = self.current_byte_offset();
        if off > u32::MAX as u64 {
            return Err("rev offset overflow".into());
        }
        self.idxf.write_all(&(off as u32).to_le_bytes())?;
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
        self.bw.byte_align();
        let off = self.current_byte_offset();
        if off > u32::MAX as u64 {
            return Err("rev offset overflow".into());
        }
        self.idxf.write_all(&(off as u32).to_le_bytes())?;
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
        self.current_count = self
            .current_count
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
        self.finish_current_id()?;
        let mut revf = self.bw.finish()?;
        revf.flush()?;
        self.idxf.flush()?;
        self.cntf.flush()?;
        Ok(())
    }
}
