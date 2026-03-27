use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use corp::corp::{Attr, Corpus, rebase_path};
use corp::corpconf::Block;
use corp::writerev_sparse;
use pico_args::Arguments;

fn usage(prog: &str) {
    eprintln!("usage: {prog} corpus dynattr");
}

fn add_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn parse_attr_block(conf: &Block, name: &str) -> Result<Block, Box<dyn std::error::Error>> {
    if let Some((sname, aname)) = name.split_once('.') {
        let s = conf.structure(sname).ok_or("structure not found")?;
        Ok(s.attribute(aname).ok_or("attribute not found")?.clone())
    } else {
        Ok(conf.attribute(name).ok_or("attribute not found")?.clone())
    }
}

fn output_base(corpus: &Corpus, attr_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some((sname, aname)) = attr_name.split_once('.') {
        if let Some(sconf) = corpus.conf.structure(sname) {
            if let Some(spath) = sconf.value("PATH") {
                let rebased = rebase_path(&corpus.name, spath)?;
                return Ok(PathBuf::from(rebased).join(aname));
            }
        }
    }
    Ok(PathBuf::from(&corpus.path).join(attr_name))
}

struct LexWriter {
    base: PathBuf,
    lex: BufWriter<File>,
    idx: BufWriter<File>,
    map: HashMap<String, u32>,
    bytes: u32,
}

impl LexWriter {
    fn new(base: &Path) -> Result<LexWriter, Box<dyn std::error::Error>> {
        Ok(LexWriter {
            base: base.to_path_buf(),
            lex: BufWriter::new(File::create(add_suffix(base, ".lex"))?),
            idx: BufWriter::new(File::create(add_suffix(base, ".lex.idx"))?),
            map: HashMap::new(),
            bytes: 0,
        })
    }

    fn id_for(&mut self, value: &str) -> Result<u32, Box<dyn std::error::Error>> {
        if let Some(&id) = self.map.get(value) {
            return Ok(id);
        }
        let id = self.map.len() as u32;
        self.map.insert(value.to_string(), id);
        self.idx.write_all(&self.bytes.to_le_bytes())?;
        self.lex.write_all(value.as_bytes())?;
        self.lex.write_all(&[0])?;
        self.bytes = self
            .bytes
            .checked_add(value.len() as u32 + 1)
            .ok_or("lexicon offset overflow")?;
        Ok(id)
    }

    fn size(&self) -> usize {
        self.map.len()
    }

    fn finalize(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.lex.flush()?;
        self.idx.flush()?;
        drop(self.lex);
        drop(self.idx);

        let lex_bytes = std::fs::read(add_suffix(&self.base, ".lex"))?;
        let idx_bytes = std::fs::read(add_suffix(&self.base, ".lex.idx"))?;
        let mut offsets = Vec::new();
        let mut off = 0;
        while off + 4 <= idx_bytes.len() {
            offsets.push(u32::from_le_bytes([
                idx_bytes[off],
                idx_bytes[off + 1],
                idx_bytes[off + 2],
                idx_bytes[off + 3],
            ]));
            off += 4;
        }

        let mut pairs: Vec<(String, u32)> = Vec::with_capacity(offsets.len());
        for (id, &ofs) in offsets.iter().enumerate() {
            let mut end = ofs as usize;
            while end < lex_bytes.len() && lex_bytes[end] != 0 {
                end += 1;
            }
            let s = std::str::from_utf8(&lex_bytes[ofs as usize..end])?.to_string();
            pairs.push((s, id as u32));
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));

        let mut srt = BufWriter::new(File::create(add_suffix(&self.base, ".lex.srt"))?);
        for (_, id) in pairs {
            srt.write_all(&id.to_le_bytes())?;
        }
        srt.flush()?;
        Ok(())
    }
}

fn write_ridx(base: &Path, ridx: &[u32]) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = BufWriter::new(File::create(add_suffix(base, ".lex.ridx"))?);
    for &id in ridx {
        f.write_all(&id.to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn write_freq(
    base: &Path,
    fromattr: &dyn Attr,
    rev_lists: &[Vec<u32>],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = BufWriter::new(File::create(add_suffix(base, ".freq"))?);
    let freq = fromattr.get_freq("frq")?;
    for poss in rev_lists {
        let mut total: u64 = 0;
        for &orig_id in poss {
            total = total
                .checked_add(freq.frq(orig_id))
                .ok_or("frequency overflow")?;
        }
        if total > i64::MAX as u64 {
            return Err("frequency value exceeds i64 range".into());
        }
        f.write_all(&(total as i64).to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn parse_arg1_usize(arg1: Option<&str>) -> Result<usize, Box<dyn std::error::Error>> {
    match arg1 {
        Some(v) if !v.trim().is_empty() => Ok(v.trim().parse::<usize>()?),
        _ => Ok(0),
    }
}

fn strip_last_n(input: &str, n: usize) -> String {
    let len = input.chars().count();
    let keep = len.saturating_sub(n);
    input.chars().take(keep).collect()
}

fn extract_host(url: &str) -> String {
    let s = url.trim();
    if s.is_empty() {
        return String::new();
    }

    let mut rest = if let Some((_, r)) = s.split_once("://") {
        r
    } else {
        s
    };

    if let Some((_, r)) = rest.rsplit_once('@') {
        rest = r;
    }

    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let mut host_port = &rest[..end];

    if host_port.starts_with('[') {
        if let Some(close) = host_port.find(']') {
            return host_port[..=close].to_string();
        }
        return host_port.to_string();
    }

    if let Some((h, p)) = host_port.rsplit_once(':') {
        if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            host_port = h;
        }
    }

    host_port.trim_matches('.').to_ascii_lowercase()
}

fn url2domain(url: &str, keep_components: usize) -> String {
    let host = extract_host(url);
    if host.is_empty() || keep_components == 0 {
        return host;
    }

    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || keep_components >= parts.len() {
        return host;
    }

    parts[parts.len() - keep_components..].join(".")
}

fn apply_internal(
    fun_name: &str,
    arg1: Option<&str>,
    input: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    match fun_name {
        "striplastn" => {
            let n = parse_arg1_usize(arg1)?;
            Ok(strip_last_n(input, n))
        }
        "url2domain" | "url3domain" => {
            let n = parse_arg1_usize(arg1)?;
            Ok(url2domain(input, n))
        }
        "utf8lowercase" => Ok(input.to_lowercase()),
        _ => Err(format!("unsupported internal function: {fun_name}").into()),
    }
}

fn compute_pipe_values(
    command: &str,
    fromattr: &dyn Attr,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let id_range = fromattr.id_range();
    let mut source_vals = Vec::with_capacity(id_range as usize);
    for id in 0..id_range {
        source_vals.push(fromattr.id2str(id).to_string());
    }

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or("failed to open child stdin")?;
    let writer = thread::spawn(move || -> Result<(), std::io::Error> {
        for s in source_vals {
            stdin.write_all(s.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
        stdin.flush()?;
        Ok(())
    });

    let stdout = child.stdout.take().ok_or("failed to open child stdout")?;
    let mut values = Vec::with_capacity(id_range as usize);
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        values.push(line.clone());
    }

    writer.join().map_err(|_| "pipe writer thread panicked")??;

    let status = child.wait()?;
    if !status.success() {
        eprintln!(
            "warning: the command '{}' exited with nonzero status: {:?}",
            command,
            status.code()
        );
    }

    if values.len() != id_range as usize {
        return Err(format!("error: expected {} values, got {}", id_range, values.len()).into());
    }

    Ok(values)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = Arguments::from_env()
        .finish()
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .unwrap_or_else(|value| value.to_string_lossy().into_owned())
        })
        .collect();
    if args.len() < 2 {
        usage("mkdynattr");
        return Err("invalid arguments".into());
    }

    let corpus_name = &args[0];
    let attr_name = &args[1];

    let corpus = Corpus::open(corpus_name)?;
    let attr_conf = parse_attr_block(&corpus.conf, attr_name)?;

    let dynlib = attr_conf.value("DYNLIB").ok_or("DYNLIB missing")?;
    let dynamic = attr_conf.value("DYNAMIC").ok_or("DYNAMIC missing")?;
    let from_attr_name = attr_conf.value("FROMATTR").ok_or("FROMATTR missing")?;
    let arg1 = attr_conf.value("ARG1");
    let dyntype = attr_conf.value("DYNTYPE").unwrap_or("");

    let full_from_attr = if let Some((sname, _)) = attr_name.split_once('.') {
        format!("{sname}.{from_attr_name}")
    } else {
        from_attr_name.to_string()
    };

    let fromattr = corpus.open_attribute(&full_from_attr)?;
    let id_range = fromattr.id_range();

    let out_base = output_base(&corpus, attr_name)?;

    let mut wl = LexWriter::new(&out_base)?;
    let mut ridx: Vec<u32> = Vec::with_capacity(id_range as usize);
    let mut rev_lists: Vec<Vec<u32>> = Vec::new();

    if dynlib == "internal" {
        for id in 0..id_range {
            let src = fromattr.id2str(id);
            let value = apply_internal(dynamic, arg1, src)?;
            let did = wl.id_for(&value)?;
            while rev_lists.len() <= did as usize {
                rev_lists.push(Vec::new());
            }
            rev_lists[did as usize].push(id);
            ridx.push(did);
        }
    } else if dynlib == "pipe" {
        let values = compute_pipe_values(dynamic, fromattr.as_ref())?;
        for (id, value) in values.into_iter().enumerate() {
            let did = wl.id_for(&value)?;
            while rev_lists.len() <= did as usize {
                rev_lists.push(Vec::new());
            }
            rev_lists[did as usize].push(id as u32);
            ridx.push(did);
        }
    } else {
        return Err(format!("unsupported DYNLIB: {dynlib}").into());
    }

    if rev_lists.len() != wl.size() {
        return Err("internal consistency error: rev size != lex size".into());
    }

    wl.finalize()?;
    write_ridx(&out_base, &ridx)?;
    if rev_lists.is_empty() {
        return Err("dynamic lexicon is empty".into());
    }
    let mut rev_writer = writerev_sparse::SparseRevWriter::create(&out_base)?;
    for (id, poss) in rev_lists.iter().enumerate() {
        rev_writer.put_batch(id as u32, poss.iter().copied().map(u64::from))?;
    }
    rev_writer.finish()?;
    // Kept for compatibility with toolchains that expect this file to exist.
    let _cnt64 = File::create(add_suffix(&out_base, ".rev.cnt64"))?;

    if dyntype == "freq" {
        write_freq(&out_base, fromattr.as_ref(), &rev_lists)?;
    } else if (id_range as u64) > (100u64.saturating_mul(rev_lists.len() as u64)) {
        eprintln!(
            "warning: the ratio between the dynamic attribute lexicon and original lexicon is more than 1:100"
        );
        eprintln!("consider changing DYNTYPE to 'freq'");
        eprintln!("Source attribute lexicon size: {id_range}");
        eprintln!("Dynamic attribute lexicon size: {}", rev_lists.len());
    }

    Ok(())
}
