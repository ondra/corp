use std::env;
use std::path::Path;

use corp::corp::Corpus;
use corp::rev;
use corp::text::{self, Text};
use corp::writerev_dense;
use corp::writerev_sparse;
use corp::writerev_temp;

const USE_DELTA_DENSE_REV: bool = true;
const CHUNK_BYTES: usize = 32 * 1024 * 1024;
const MAX_OPEN_RUNS: usize = 32;
const TEMP_ALIGNMULT: usize = 1;


fn add_suffix(base: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
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

fn write_temp_rev_from_pairs(
    base: &Path,
    max_id: u32,
    pairs: &[(u32, u64)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = writerev_temp::TempRevWriter::create(base, TEMP_ALIGNMULT)?;
    let mut prev: Option<(u32, u64)> = None;
    for &(id, pos) in pairs {
        if let Some((pid, ppos)) = prev {
            if id < pid || (id == pid && pos <= ppos) {
                return Err("pairs not sorted by id/position".into());
            }
        }
        writer.put(id, pos)?;
        prev = Some((id, pos));
    }
    writer.fill_to(max_id)?;
    writer.finish()
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

    if USE_DELTA_DENSE_REV {
        let mut writer = writerev_dense::DenseRevWriter::create(base)?;
        for id in 0..=max_id {
            for (run, reader) in runs.iter().zip(readers.iter()) {
                if id > run.max_id || reader.count(id) == 0 {
                    continue;
                }
                for pos in reader.id2poss(id) {
                    writer.put(id, pos)?;
                }
            }
        }
        writer.finish()
    } else {
        let mut writer = writerev_sparse::SparseRevWriter::create(base)?;
        for id in 0..=max_id {
            for (run, reader) in runs.iter().zip(readers.iter()) {
                if id > run.max_id || reader.count(id) == 0 {
                    continue;
                }
                for pos in reader.id2poss(id) {
                    writer.put(id, pos)?;
                }
            }
        }
        writer.finish()
    }
}

fn write_temp_rev_from_runs(
    base: &Path,
    max_id: u32,
    runs: &[RunInfo],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut readers: Vec<Box<dyn rev::Rev + Sync + Send>> = Vec::with_capacity(runs.len());
    for run in runs {
        let run_base = run.base.to_str().ok_or("bad run path")?;
        readers.push(rev::open(run_base)?);
    }

    let mut writer = writerev_temp::TempRevWriter::create(base, TEMP_ALIGNMULT)?;
    for id in 0..=max_id {
        for (run, reader) in runs.iter().zip(readers.iter()) {
            if id > run.max_id || reader.count(id) == 0 {
                continue;
            }
            writer.put_batch(id, reader.id2poss(id))?;
        }
    }
    writer.fill_to(max_id)?;
    writer.finish()
}

fn remove_temp_run(run: &RunInfo) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::remove_file(add_suffix(&run.base, ".rev"))?;
    std::fs::remove_file(add_suffix(&run.base, ".rev.idx"))?;
    std::fs::remove_file(add_suffix(&run.base, ".rev.cnt"))?;
    let _ = std::fs::remove_file(add_suffix(&run.base, ".rev.cnt64"));
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
        write_temp_rev_from_runs(&out_base, max_id, chunk)?;
        merged.push(RunInfo { base: out_base, max_id });
        for run in chunk {
            remove_temp_run(run)?;
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
        write_temp_rev_from_pairs(&run_base, chunk_max_id, &pairs)?;
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

    let final_base = temp_base(&base, seq);
    let final_run = RunInfo { base: final_base, max_id: global_max_id };
    write_rev_from_runs(&final_run.base, global_max_id, &runs)?;
    for run in &runs {
        remove_temp_run(run)?;
    }
    move_run_to_final(&base, &final_run)?;
    Ok(())
}
