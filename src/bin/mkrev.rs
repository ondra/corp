use std::env;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use corp::corp::Corpus;
use corp::rev;
use corp::text::{self, Text};
use corp::wrbits::BitsWriter;

const REV_MAGIC: [u8; 6] = [0xa3, b'f', b'i', b'n', b'D', b'R'];
const REV_DENSE_MAGIC: [u8; 6] = [0xa8, b'f', b'i', b'n', b'D', b'R'];
const USE_DELTA_DENSE_REV: bool = true;
const CHUNK_BYTES: usize = 32 * 1024 * 1024;
const MAX_OPEN_RUNS: usize = 32;


fn add_suffix(base: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
}

fn write_rev_delta_with<F>(
    base: &Path,
    max_id: u32,
    mut write_positions: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(u32, &mut BitsWriter) -> Result<u32, Box<dyn std::error::Error>>,
{
    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
    f.write_all(&REV_MAGIC)?;
    f.flush()?;

    let mut hbw = BitsWriter::new(f);
    hbw.delta(2);
    let mut f = hbw.finish()?;
    let header_end = f.seek(SeekFrom::Current(0))?;
    f.seek(SeekFrom::Start(header_end))?;
    let mut bw = BitsWriter::new(f);

    let mut idx = Vec::with_capacity((max_id as usize) + 1);
    let mut cnts = Vec::with_capacity((max_id as usize) + 1);
    for id in 0..=max_id {
        bw.byte_align();
        let bitpos = bw.bits_written();
        let byte_off = header_end as u64 + (bitpos / 8);
        if byte_off > u32::MAX as u64 {
            return Err("rev offset overflow".into());
        }
        idx.push(byte_off as u32);
        let cnt = write_positions(id, &mut bw)?;
        cnts.push(cnt);
    }
    let _f = bw.finish()?;

    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev.idx"))?);
    for off in idx {
        f.write_all(&off.to_le_bytes())?;
    }
    f.flush()?;

    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev.cnt"))?);
    for cnt in cnts {
        f.write_all(&cnt.to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn write_rev_dense_with<F>(
    base: &Path,
    max_id: u32,
    mut write_positions: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(u32, &mut BitsWriter) -> Result<u32, Box<dyn std::error::Error>>,
{
    let mut f = BufWriter::new(File::create(add_suffix(base, ".rev"))?);
    f.write_all(&REV_DENSE_MAGIC)?;
    f.flush()?;
    let data_start = f.seek(SeekFrom::Current(0))?;
    let mut bw = BitsWriter::new(f);

    let mut byte_offsets: Vec<u32> = Vec::with_capacity((max_id as usize) + 1);
    let mut counts: Vec<u32> = Vec::with_capacity((max_id as usize) + 1);
    for id in 0..=max_id {
        bw.byte_align();
        let bitpos = bw.bits_written();
        let byte_off = data_start as u64 + (bitpos / 8);
        if byte_off > u32::MAX as u64 {
            return Err("rev dense offset overflow".into());
        }
        byte_offsets.push(byte_off as u32);
        let cnt = write_positions(id, &mut bw)?;
        counts.push(cnt);
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
            let cnt = counts[i] as u64 + 1;
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

fn write_rev_with<F>(
    base: &Path,
    max_id: u32,
    write_positions: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(u32, &mut BitsWriter) -> Result<u32, Box<dyn std::error::Error>>,
{
    if USE_DELTA_DENSE_REV {
        write_rev_dense_with(base, max_id, write_positions)
    } else {
        write_rev_delta_with(base, max_id, write_positions)
    }
}

fn temp_base(base: &Path, seq: u32) -> std::path::PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(format!("#{}", seq));
    std::path::PathBuf::from(s)
}

#[derive(Debug)]
struct RunInfo {
    base: std::path::PathBuf,
    max_id: u32,
}

enum IdIter<'a> {
    Delta(text::DeltaIter<'a>),
    Int(text::IntIter<'a>),
}

impl IdIter<'_> {
    fn next_id(&mut self) -> Option<u32> {
        match self {
            IdIter::Delta(it) => it.next(),
            IdIter::Int(it) => it.next(),
        }
    }
}

fn make_iter<'a>(text: &'a dyn Text, pos: u64) -> Result<IdIter<'a>, Box<dyn std::error::Error>> {
    if let Some(it) = text.posat(pos) {
        Ok(IdIter::Delta(it))
    } else if let Some(it) = text.structat(pos) {
        Ok(IdIter::Int(it))
    } else {
        Err("text type not supported".into())
    }
}

fn write_rev_from_pairs(
    base: &Path,
    max_id: u32,
    pairs: &[(u32, u64)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pair_idx = 0usize;
    write_rev_with(base, max_id, |id, bw| {
        let mut last: Option<u64> = None;
        let mut count: u32 = 0;
        while pair_idx < pairs.len() && pairs[pair_idx].0 == id {
            let pos = pairs[pair_idx].1;
            let gap = match last {
                None => pos.checked_add(1).ok_or("rev position overflow")?,
                Some(prev) => pos.saturating_sub(prev),
            };
            if gap == 0 {
                return Err("invalid zero gap in rev".into());
            }
            bw.delta(gap);
            last = Some(pos);
            count += 1;
            if count == u32::MAX {
                return Err("rev count overflow".into());
            }
            pair_idx += 1;
        }
        if pair_idx < pairs.len() && pairs[pair_idx].0 < id {
            return Err("pairs not sorted by id".into());
        }
        Ok(count)
    })
}

fn write_rev_from_runs(
    base: &Path,
    max_id: u32,
    runs: &[RunInfo],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut readers: Vec<Box<dyn rev::Rev + Sync + Send>> = Vec::with_capacity(runs.len());
    for run in runs {
        let run_base = run.base.to_str().ok_or("bad run path")?;
        readers.push(rev::open(run_base)?);
    }

    write_rev_with(base, max_id, |id, bw| {
        let mut last: Option<u64> = None;
        let mut count: u32 = 0;
        for (run, reader) in runs.iter().zip(readers.iter()) {
            if id > run.max_id {
                continue;
            }
            if reader.count(id) == 0 {
                continue;
            }
            let mut it = reader.id2poss(id);
            while let Some(pos) = it.next() {
                let gap = match last {
                    None => pos.checked_add(1).ok_or("rev position overflow")?,
                    Some(prev) => pos.saturating_sub(prev),
                };
                if gap == 0 {
                    return Err("invalid zero gap in rev".into());
                }
                bw.delta(gap);
                last = Some(pos);
                count += 1;
                if count == u32::MAX {
                    return Err("rev count overflow".into());
                }
            }
        }
        Ok(count)
    })
}

fn remove_run(run: &RunInfo) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::remove_file(add_suffix(&run.base, ".rev"))?;
    if USE_DELTA_DENSE_REV {
        std::fs::remove_file(add_suffix(&run.base, ".rev.idx0"))?;
        std::fs::remove_file(add_suffix(&run.base, ".rev.idx1"))?;
    } else {
        std::fs::remove_file(add_suffix(&run.base, ".rev.idx"))?;
        std::fs::remove_file(add_suffix(&run.base, ".rev.cnt"))?;
    }
    Ok(())
}

fn move_run_to_final(
    base: &Path,
    run: &RunInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::rename(add_suffix(&run.base, ".rev"), add_suffix(base, ".rev"))?;
    if USE_DELTA_DENSE_REV {
        std::fs::rename(add_suffix(&run.base, ".rev.idx0"), add_suffix(base, ".rev.idx0"))?;
        std::fs::rename(add_suffix(&run.base, ".rev.idx1"), add_suffix(base, ".rev.idx1"))?;
    } else {
        std::fs::rename(add_suffix(&run.base, ".rev.idx"), add_suffix(base, ".rev.idx"))?;
        std::fs::rename(add_suffix(&run.base, ".rev.cnt"), add_suffix(base, ".rev.cnt"))?;
    }
    Ok(())
}

fn merge_runs_pass(
    base: &Path,
    runs: &[RunInfo],
    seq: &mut u32,
) -> Result<Vec<RunInfo>, Box<dyn std::error::Error>> {
    let mut merged: Vec<RunInfo> = Vec::new();
    for chunk in runs.chunks(MAX_OPEN_RUNS) {
        let max_id = chunk.iter().map(|r| r.max_id).max().unwrap_or(0);
        let out_base = temp_base(base, *seq);
        *seq += 1;
        write_rev_from_runs(&out_base, max_id, chunk)?;
        merged.push(RunInfo { base: out_base, max_id });
        for run in chunk {
            remove_run(run)?;
        }
    }
    Ok(merged)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("Usage: mkrev <config> <attribute>");
        eprintln!("  config is a corpus configuration path or name");
        eprintln!("  attribute is the attribute name (e.g., word or s.attr)");
        return Ok(());
    }
    let corpname = args.remove(0);
    let attrname = args.remove(0);
    let corp = Corpus::open(&corpname)?;
    let base = std::path::PathBuf::from(corp.path.clone() + "/" + &attrname);
    let text = corp.open_text(base.to_str().unwrap(), corp.get_text_storage_type(&attrname)?)?;
    let size = text.size() as u64;

    let chunk_pairs = std::cmp::max(1, CHUNK_BYTES / std::mem::size_of::<(u32, u64)>());
    let mut runs: Vec<RunInfo> = Vec::new();
    let mut global_max_id: u32 = 0;
    let mut seq: u32 = 0;
    let mut pos: u64 = 0;

    while pos < size {
        let remaining = (size - pos) as usize;
        let chunk_len = std::cmp::min(chunk_pairs, remaining);
        let mut pairs: Vec<(u32, u64)> = Vec::with_capacity(chunk_len);
        let mut chunk_max_id: u32 = 0;
        let mut iter = make_iter(text.as_ref(), pos)?;
        for _ in 0..chunk_len {
            let id = iter.next_id().ok_or("text underflow")?;
            pairs.push((id, pos));
            if id > chunk_max_id {
                chunk_max_id = id;
            }
            pos += 1;
        }
        if pairs.is_empty() {
            break;
        }
        pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let run_base = temp_base(&base, seq);
        seq += 1;
        write_rev_from_pairs(&run_base, chunk_max_id, &pairs)?;
        runs.push(RunInfo { base: run_base, max_id: chunk_max_id });
        if chunk_max_id > global_max_id {
            global_max_id = chunk_max_id;
        }
    }

    if runs.is_empty() {
        return Err("empty text".into());
    }

    while runs.len() > 1 {
        runs = merge_runs_pass(&base, &runs, &mut seq)?;
    }
    if runs[0].max_id < global_max_id {
        let final_base = temp_base(&base, seq);
        write_rev_from_runs(&final_base, global_max_id, &runs)?;
        remove_run(&runs[0])?;
        runs[0] = RunInfo { base: final_base, max_id: global_max_id };
    }
    move_run_to_final(&base, &runs[0])?;
    Ok(())
}
