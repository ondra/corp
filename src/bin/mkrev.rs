use std::env;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use corp::text::{self, Text};
use corp::wrbits::BitsWriter;

const REV_MAGIC: [u8; 6] = [0xa3, b'f', b'i', b'n', b'D', b'R'];
const REV_DENSE_MAGIC: [u8; 6] = [0xa8, b'f', b'i', b'n', b'D', b'R'];
const USE_DELTA_DENSE_REV: bool = true;


fn add_suffix(base: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
}

fn write_rev_delta(
    base: &Path,
    positions: &[Vec<u32>],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
    f.write_all(&REV_MAGIC)?;
    f.flush()?;

    let mut hbw = BitsWriter::new(f);
    hbw.delta(2);
    let mut f = hbw.finish()?;
    let header_end = f.seek(SeekFrom::Current(0))?;
    f.seek(SeekFrom::Start(header_end))?;
    let mut bw = BitsWriter::new(f);

    let mut idx = Vec::with_capacity(positions.len());
    for poslist in positions {
        bw.byte_align();
        let bitpos = bw.bits_written();
        let byte_off = header_end as u64 + (bitpos / 8);
        if byte_off > u32::MAX as u64 {
            return Err("rev offset overflow".into());
        }
        idx.push(byte_off as u32);
        let mut last: i64 = -1;
        for &p in poslist {
            let gap = (p as i64 - last) as u64;
            if gap == 0 {
                return Err("invalid zero gap in rev".into());
            }
            bw.delta(gap);
            last = p as i64;
        }
    }
    let _f = bw.finish()?;

    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev.idx"))?);
    for off in idx {
        f.write_all(&off.to_le_bytes())?;
    }
    f.flush()?;

    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev.cnt"))?);
    for poslist in positions {
        let cnt = poslist.len() as u32;
        f.write_all(&cnt.to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn write_rev_dense(
    base: &Path,
    positions: &[Vec<u32>],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
    f.write_all(&REV_DENSE_MAGIC)?;
    f.flush()?;
    let data_start = f.seek(SeekFrom::Current(0))?;
    let mut bw = BitsWriter::new(f);

    let mut byte_offsets: Vec<u32> = Vec::with_capacity(positions.len());
    for poslist in positions {
        bw.byte_align();
        let bitpos = bw.bits_written();
        let byte_off = data_start as u64 + (bitpos / 8);
        if byte_off > u32::MAX as u64 {
            return Err("rev dense offset overflow".into());
        }
        byte_offsets.push(byte_off as u32);

        let mut last: i64 = -1;
        for &p in poslist {
            let gap = (p as i64 - last) as u64;
            if gap == 0 {
                return Err("invalid zero gap in rev".into());
            }
            bw.delta(gap);
            last = p as i64;
        }
    }
    let _f = bw.finish()?;

    let mut idx0: Vec<u32> = Vec::new();
    let idx1 = BufWriter::new(File::create(add_suffix(base, ".rev.idx1"))?);
    let mut bw1 = BitsWriter::new(idx1);
    let mut block_start = 0usize;
    while block_start < byte_offsets.len() {
        bw1.byte_align();
        let idx1_byte = bw1.bits_written() / 8;
        if idx1_byte > u32::MAX as u64 {
            return Err("rev dense idx1 overflow".into());
        }
        idx0.push(idx1_byte as u32);

        let mut last_off: u32 = 0;
        let end = std::cmp::min(block_start + 64, byte_offsets.len());
        for i in block_start..end {
            let off = byte_offsets[i];
            let delta = off.wrapping_sub(last_off);
            if delta == 0 {
                return Err("invalid zero delta in rev dense".into());
            }
            bw1.delta(delta as u64);
            let cnt = positions[i].len() as u64 + 1;
            bw1.gamma(cnt);
            last_off = off;
        }
        bw1.delta(1);
        bw1.gamma(1);
        block_start += 64;
    }
    let mut idx1_file = bw1.finish()?;
    idx1_file.flush()?;
    let idx1_end = idx1_file.seek(SeekFrom::Current(0))?;
    if idx1_end > u32::MAX as u64 {
        return Err("rev dense idx1 overflow".into());
    }
    idx0.push(idx1_end as u32);

    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev.idx0"))?);
    for off in idx0 {
        f.write_all(&off.to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn write_rev(base: &Path, positions: &[Vec<u32>]) -> Result<(), Box<dyn std::error::Error>> {
    if USE_DELTA_DENSE_REV {
        write_rev_dense(base, positions)
    } else {
        write_rev_delta(base, positions)
    }
}

fn open_text(base: &Path) -> Result<Box<dyn Text>, Box<dyn std::error::Error>> {
    if add_suffix(base, ".text.off").exists() {
        Ok(Box::new(text::GigaDelta::open(base.to_str().ok_or("bad path")?)?))
    } else if add_suffix(base, ".text.seg").exists() {
        Ok(Box::new(text::Delta::open(base.to_str().ok_or("bad path")?)?))
    } else {
        Ok(Box::new(text::Int::open(base.to_str().ok_or("bad path")?)?))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: mkrev <base>");
        eprintln!("  base is the attribute base path without extension (e.g., /path/word)");
        return Ok(());
    }
    let base = std::path::PathBuf::from(args.remove(0));

    let text = open_text(&base)?;
    let size = text.size() as u32;
    let mut positions: Vec<Vec<u32>> = Vec::new();

    if let Some(mut it) = text.posat(0) {
        for pos in 0..size {
            let id = it.next().ok_or("text underflow")?;
            let idx = id as usize;
            if idx >= positions.len() {
                positions.resize_with(idx + 1, Vec::new);
            }
            positions[idx].push(pos);
        }
    } else if let Some(mut it) = text.structat(0) {
        for pos in 0..size {
            let id = it.next().ok_or("text underflow")?;
            let idx = id as usize;
            if idx >= positions.len() {
                positions.resize_with(idx + 1, Vec::new);
            }
            positions[idx].push(pos);
        }
    } else {
        return Err("text type not supported".into());
    }

    write_rev(&base, &positions)?;
    Ok(())
}
