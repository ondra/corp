use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use chrono::Utc;

use corpconf::Block;
use corp::corp::rebase_path;
use corp::wrbits::BitsWriter;

const TEXT_MAGIC: [u8; 6] = [0xa3, b'f', b'i', b'n', b'D', b'T'];
const INT_MAGIC: [u8; 6] = [0xa3, b'f', b'i', b'n', b'I', b'T'];
const DEFAULT_SEGMENT_SIZE: usize = 128;

const DATA_ALIGN: u64 = 32;
const STATUS_EVERY_LINES: u64 = 10_000_000;
const ENC_ERR_MAX: i64 = 100;
const WARN_VERBOSE: bool = false;

#[derive(Clone, Copy, Debug)]
enum TextType {
    Delta,
    Int,
    GigaDelta,
}

struct EncErr {
    name: &'static str,
    count: i64,
}

impl EncErr {
    fn new(name: &'static str) -> EncErr {
        EncErr { name, count: 0 }
    }

    fn emit(&mut self, line: u64, msg: &str) {
        if self.count < ENC_ERR_MAX || ENC_ERR_MAX == -1 {
            eprintln!("line {}: warning: {}", line, msg);
        }
        if self.count == ENC_ERR_MAX - 1 && ENC_ERR_MAX != -1 {
            eprintln!("There were already {} similar errors in the input", ENC_ERR_MAX);
            eprintln!("further errors will be suppressed and a summary will be");
            eprintln!("provided at the end of the compilation.");
            eprintln!("Use -v to emit all occurrences.");
        }
        self.count += 1;
    }

    fn summary(&self) {
        if WARN_VERBOSE || self.count > 0 {
            eprintln!("{} times: warning type '{}'", self.count, self.name);
        }
    }
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

trait TextWriter {
    fn push(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>>;
    fn finalize(self: Box<Self>) -> Result<(), Box<dyn std::error::Error>>;
}

struct DeltaTextWriter {
    base: PathBuf,
    bw: Option<BitsWriter>,
    seg: BufWriter<File>,
    segment_size: usize,
    data_start: u64,
    count: u64,
}

impl DeltaTextWriter {
    fn new(base: &Path, segment_size: usize) -> Result<DeltaTextWriter, Box<dyn std::error::Error>> {
        let mut f = BufWriter::new(File::create(add_suffix(base, ".text"))?);
        f.write_all(&TEXT_MAGIC)?;
        f.write_all(&[0u8; 10])?;
        f.flush()?;

        f.seek(SeekFrom::Start(16))?;
        let mut hbw = BitsWriter::new(f);
        hbw.delta(segment_size as u64 + 1);
        hbw.delta(0 + 1);
        let mut f = hbw.finish()?;
        let header_end = f.seek(SeekFrom::Current(0))?;
        let data_start = align_writer(&mut f, header_end, DATA_ALIGN)?;
        f.seek(SeekFrom::Start(data_start))?;

        Ok(DeltaTextWriter {
            base: base.to_path_buf(),
            bw: Some(BitsWriter::new(f)),
            seg: BufWriter::new(File::create(add_suffix(base, ".text.seg"))?),
            segment_size,
            data_start,
            count: 0,
        })
    }

    fn finish_data(&mut self) -> Result<(Vec<u32>, u64), Box<dyn std::error::Error>> {
        let bw = self.bw.take().ok_or("delta writer already finished")?;
        let total_bits = bw.bits_written();
        let _f = bw.finish()?;
        let end_bitpos = self.data_start * 8 + total_bits;
        if end_bitpos > u32::MAX as u64 {
            return Err("text segment offset overflow".into());
        }
        self.seg.write_all(&(end_bitpos as u32).to_le_bytes())?;
        self.seg.flush()?;
        Ok((Vec::new(), self.count))
    }
}

impl TextWriter for DeltaTextWriter {
    fn push(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        if (self.count as usize) % self.segment_size == 0 {
            let bw = self.bw.as_ref().ok_or("delta writer already finished")?;
            let bitpos = self.data_start * 8 + bw.bits_written();
            if bitpos > u32::MAX as u64 {
                return Err("text segment offset overflow".into());
            }
            self.seg.write_all(&(bitpos as u32).to_le_bytes())?;
        }
        let bw = self.bw.as_mut().ok_or("delta writer already finished")?;
        bw.delta(id as u64 + 1);
        self.count += 1;
        Ok(())
    }

    fn finalize(mut self: Box<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let (_seg, count) = self.finish_data()?;

        let file = OpenOptions::new()
            .write(true)
            .open(add_suffix(&self.base, ".text"))?;
        let mut f = BufWriter::new(file);
        f.seek(SeekFrom::Start(16))?;
        let mut hbw = BitsWriter::new(f);
        hbw.delta(self.segment_size as u64 + 1);
        hbw.delta(count + 1);
        let mut f = hbw.finish()?;
        f.flush()?;
        Ok(())
    }
}

struct GigaDeltaTextWriter {
    base: PathBuf,
    bw: Option<BitsWriter>,
    offsets: BufWriter<File>,
    segments: BufWriter<File>,
    data_start: u64,
    count: u64,
    seg_base_bits: u64,
}

impl GigaDeltaTextWriter {
    fn new(base: &Path) -> Result<GigaDeltaTextWriter, Box<dyn std::error::Error>> {
        let mut f = BufWriter::new(File::create(add_suffix(base, ".text"))?);
        f.write_all(&TEXT_MAGIC)?;
        f.write_all(&[0u8; 10])?;
        f.flush()?;

        f.seek(SeekFrom::Start(16))?;
        let mut hbw = BitsWriter::new(f);
        hbw.delta(64 + 1);
        hbw.delta(0 + 1);
        let mut f = hbw.finish()?;
        let header_end = f.seek(SeekFrom::Current(0))?;
        let data_start = align_writer(&mut f, header_end, DATA_ALIGN)?;
        f.seek(SeekFrom::Start(data_start))?;

        Ok(GigaDeltaTextWriter {
            base: base.to_path_buf(),
            bw: Some(BitsWriter::new(f)),
            offsets: BufWriter::new(File::create(add_suffix(base, ".text.off"))?),
            segments: BufWriter::new(File::create(add_suffix(base, ".text.seg"))?),
            data_start,
            count: 0,
            seg_base_bits: 0,
        })
    }

    fn finish_data(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
        let bw = self.bw.take().ok_or("gigadelta writer already finished")?;
        let _f = bw.finish()?;
        self.offsets.flush()?;
        self.segments.flush()?;
        Ok(self.count)
    }
}

impl TextWriter for GigaDeltaTextWriter {
    fn push(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        let i = self.count as usize;
        if i % (64 * 16) == 0 {
            let bw = self.bw.as_ref().ok_or("gigadelta writer already finished")?;
            let bitpos = self.data_start * 8 + bw.bits_written();
            let base_block = bitpos / (2048 * 8);
            if base_block > u32::MAX as u64 {
                return Err("text segment offset overflow".into());
            }
            self.segments.write_all(&(base_block as u32).to_le_bytes())?;
            self.seg_base_bits = base_block * (2048 * 8);
        }
        if i % 64 == 0 {
            let bw = self.bw.as_ref().ok_or("gigadelta writer already finished")?;
            let bitpos = self.data_start * 8 + bw.bits_written();
            let rel = bitpos - self.seg_base_bits;
            if rel > u16::MAX as u64 {
                return Err("text offset overflow".into());
            }
            self.offsets.write_all(&(rel as u16).to_le_bytes())?;
        }
        let bw = self.bw.as_mut().ok_or("gigadelta writer already finished")?;
        bw.delta(id as u64 + 1);
        self.count += 1;
        Ok(())
    }

    fn finalize(mut self: Box<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let count = self.finish_data()?;

        let file = OpenOptions::new()
            .write(true)
            .open(add_suffix(&self.base, ".text"))?;
        let mut f = BufWriter::new(file);
        f.seek(SeekFrom::Start(16))?;
        let mut hbw = BitsWriter::new(f);
        hbw.delta(64 + 1);
        hbw.delta(count + 1);
        let mut f = hbw.finish()?;
        f.flush()?;
        Ok(())
    }
}

struct IntTextWriter {
    base: PathBuf,
    f: BufWriter<File>,
    count: u64,
}

impl IntTextWriter {
    fn new(base: &Path) -> Result<IntTextWriter, Box<dyn std::error::Error>> {
        let mut f = BufWriter::new(File::create(add_suffix(base, ".text"))?);
        f.write_all(&INT_MAGIC)?;
        f.write_all(&[0u8; 10])?;
        Ok(IntTextWriter {
            base: base.to_path_buf(),
            f,
            count: 0,
        })
    }
}

impl TextWriter for IntTextWriter {
    fn push(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.f.write_all(&id.to_le_bytes())?;
        self.count += 1;
        Ok(())
    }

    fn finalize(mut self: Box<Self>) -> Result<(), Box<dyn std::error::Error>> {
        self.f.flush()?;
        let _ = &self.base;
        Ok(())
    }
}

struct AttrWriter {
    lex: LexWriter,
    text: Box<dyn TextWriter>,
    default_value: String,
}

impl AttrWriter {
    fn new(
        _name: &str,
        base: &Path,
        text_type: TextType,
        segment_size: usize,
        default_value: String,
    ) -> Result<AttrWriter, Box<dyn std::error::Error>> {
        let lex = LexWriter::new(base)?;
        let text: Box<dyn TextWriter> = match text_type {
            TextType::Delta => Box::new(DeltaTextWriter::new(base, segment_size)?),
            TextType::Int => Box::new(IntTextWriter::new(base)?),
            TextType::GigaDelta => Box::new(GigaDeltaTextWriter::new(base)?),
        };
        Ok(AttrWriter { lex, text, default_value })
    }

    fn push_value(&mut self, value: &str, pos: u32) -> Result<(), Box<dyn std::error::Error>> {
        let id = self.lex.id_for(value)?;
        self.text.push(id)?;
        let _ = pos;
        Ok(())
    }

}

struct StructAttrWriter {
    name: String,
    lex: LexWriter,
    text: IntTextWriter,
    default_value: String,
}

impl StructAttrWriter {
    fn new(
        name: &str,
        base: &Path,
        default_value: String,
    ) -> Result<StructAttrWriter, Box<dyn std::error::Error>> {
        Ok(StructAttrWriter {
            name: name.to_string(),
            lex: LexWriter::new(base)?,
            text: IntTextWriter::new(base)?,
            default_value,
        })
    }

    fn id_for(&mut self, value: &str) -> Result<u32, Box<dyn std::error::Error>> {
        let id = self.lex.id_for(value)?;
        Ok(id)
    }

    fn push_value(&mut self, id: u32, _struct_pos: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.text.push(id)?;
        Ok(())
    }

}

struct StructWriter {
    type64: bool,
    rng: BufWriter<File>,
    count: u32,
    attrs: Vec<StructAttrWriter>,
    last_start_pos: Option<u64>,
    pending_empty_pos: Option<u64>,
    pending_empty_vals: Option<Vec<String>>,
}

struct OpenStruct {
    name: String,
    start: u64,
    attr_values: Vec<String>,
}

#[derive(Debug)]
enum Tag {
    Start { name: String, attrs: HashMap<String, String>, self_close: bool },
    End { name: String },
}

fn parse_tag(line: &str) -> Option<Tag> {
    let s = line.trim();
    if !(s.starts_with('<') && s.ends_with('>')) {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    if inner.starts_with('/') {
        let name = inner[1..].trim().to_string();
        if name.is_empty() {
            return None;
        }
        return Some(Tag::End { name });
    }
    let self_close = inner.ends_with('/');
    let content = if self_close {
        inner[..inner.len() - 1].trim()
    } else {
        inner.trim()
    };
    if content.is_empty() {
        return None;
    }
    let mut parts = content.split_whitespace();
    let name = parts.next()?.to_string();
    let attrs_str = content[name.len()..].trim();
    let attrs = parse_attrs(attrs_str);
    Some(Tag::Start { name, attrs, self_close })
}

fn parse_attrs(mut s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }
        let key_end = s.find(|c: char| c == '=' || c.is_whitespace());
        let key_end = match key_end {
            Some(v) => v,
            None => break,
        };
        let key = s[..key_end].trim();
        s = &s[key_end..];
        s = s.trim_start();
        if !s.starts_with('=') {
            break;
        }
        s = &s[1..];
        s = s.trim_start();
        if s.starts_with('"') || s.starts_with('\'') {
            let quote = s.chars().next().unwrap();
            s = &s[1..];
            if let Some(end) = s.find(quote) {
                let val = &s[..end];
                out.insert(key.to_string(), val.to_string());
                s = &s[end + 1..];
            } else {
                break;
            }
        } else {
            let end = s.find(char::is_whitespace).unwrap_or(s.len());
            let val = &s[..end];
            out.insert(key.to_string(), val.to_string());
            s = &s[end..];
        }
    }
    out
}

fn print_usage() {
    println!("encodevert (minimal)");
    println!();
    println!("Usage:");
    println!("  encodevert <config> [input]");
    println!();
    println!("If input is omitted or '-', stdin is used.");
}

fn read_conf(path: &Path) -> Result<Block, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    Ok(corpconf::parse_conf_opt(&buf)?)
}

fn align_writer(
    writer: &mut BufWriter<File>,
    pos: u64,
    align: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    if align == 0 {
        return Ok(pos);
    }
    let rem = pos % align;
    if rem == 0 {
        return Ok(pos);
    }
    let pad = align - rem;
    writer.write_all(&vec![0u8; pad as usize])?;
    Ok(pos + pad)
}

fn add_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn timestamp_iso_utc() -> String {
    Utc::now().to_rfc3339()
}

fn attr_text_type(conf: &Block, name: &str) -> TextType {
    if let Some(attr) = conf.attribute(name) {
        match attr.value("TYPE") {
            Some("MD_MI") | Some("Int") => TextType::Int,
            Some("MD_MGD") | Some("FD_MGD") | Some("FD_FGD") | Some("NoMem") => TextType::GigaDelta,
            _ => TextType::Delta,
        }
    } else {
        TextType::Delta
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("encodevert (minimal) {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.is_empty() {
        print_usage();
        return Ok(());
    }
    let conf_path = PathBuf::from(args.remove(0));
    let input = args.get(0).cloned().unwrap_or_else(|| "-".to_string());

    let conf = read_conf(&conf_path)?;
    let out_path = conf
        .value("PATH")
        .ok_or("PATH not set in config")?;
    let out_path = rebase_path(conf_path.to_str().ok_or("bad config path")?, out_path)?;
    let out_path = PathBuf::from(out_path);
    fs::create_dir_all(&out_path)?;

    let segment_size = conf
        .value("SEGMENTSIZE")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SEGMENT_SIZE);

    let mut attrs: Vec<AttrWriter> = Vec::new();
    for name in conf.attrnames_in_order() {
        let block = conf.attribute(name).ok_or("attribute not found")?;
        if block.value("DYNAMIC").is_some() {
            continue;
        }
        let tt = attr_text_type(&conf, name);
        let default_value = block
            .value("DEFAULTVALUE")
            .unwrap_or("===NONE===")
            .to_string();
        let base = out_path.join(name);
        attrs.push(AttrWriter::new(name, &base, tt, segment_size, default_value)?);
    }

    let mut structs: HashMap<String, StructWriter> = HashMap::new();
    for sname in conf.structnames_in_order() {
        let sblock = conf.structure(sname).ok_or("structure not found")?;
        let type64 = matches!(sblock.value("TYPE"), Some("file64") | Some("map64"));
        let mut sattrs = Vec::new();
        for aname in sblock.attrnames_in_order() {
            let base = out_path.join(format!("{}.{}", sname, aname));
            let ablock = sblock.attribute(aname).ok_or("structure attribute not found")?;
            let default_value = ablock
                .value("DEFAULTVALUE")
                .unwrap_or("===NONE===")
                .to_string();
            sattrs.push(StructAttrWriter::new(aname, &base, default_value)?);
        }
        let rng = BufWriter::new(File::create(add_suffix(&out_path.join(sname), ".rng"))?);
        structs.insert(
            sname.to_string(),
            StructWriter {
                type64,
                rng,
                count: 0,
                attrs: sattrs,
                last_start_pos: None,
                pending_empty_pos: None,
                pending_empty_vals: None,
            },
        );
    }

    let stdin = io::stdin();
    let mut reader: Box<dyn BufRead> = if input == "-" {
        Box::new(stdin.lock())
    } else {
        Box::new(BufReader::new(File::open(input)?))
    };

    let mut pos: u32 = 0;
    let mut buf = String::new();
    let mut open_structs: Vec<OpenStruct> = Vec::new();
    let mut lineno: u64 = 0;
    let mut err_open_same_str = EncErr::new("structure opened multiple times on same position");
    let mut err_closing_str = EncErr::new("closing non opened structure");
    let mut err_mismatch_str = EncErr::new("mismatched closing structure");
    let mut err_unterminated = EncErr::new("unterminated structure tags");

    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(e.into()),
        }
        lineno += 1;
        if lineno % STATUS_EVERY_LINES == 0 {
            let ts = timestamp_iso_utc();
            eprintln!(
                "encodevert: status [{}] line {}, position {}",
                ts, lineno, pos
            );
        }
        while buf.ends_with('\n') || buf.ends_with('\r') {
            buf.pop();
        }
        let line = buf.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut handled_tag = false;
        if let Some(tag) = parse_tag(line) {
            match tag {
                Tag::Start { name, attrs: tag_attrs, self_close } => {
                    if let Some(sb) = structs.get_mut(&name) {
                        if let Some(pend_pos) = sb.pending_empty_pos {
                            if pend_pos != pos as u64 {
                                flush_pending_empty(sb)?;
                            }
                        }
                        if sb.last_start_pos == Some(pos as u64) {
                            err_open_same_str.emit(
                                lineno,
                                &format!(
                                    "opening structure ({}) on the same position, ignoring the previous empty one",
                                    name
                                ),
                            );
                        }
                        {
                            let mut attr_values = Vec::new();
                            for attr in &mut sb.attrs {
                                let val = tag_attrs
                                    .get(&attr.name)
                                    .map(|s| s.as_str())
                                    .unwrap_or(&attr.default_value);
                                attr_values.push(val.to_string());
                            }
                            sb.last_start_pos = Some(pos as u64);
                            if self_close {
                                if sb.pending_empty_pos == Some(pos as u64) {
                                    // replace previous empty structure at same position
                                }
                                sb.pending_empty_pos = Some(pos as u64);
                                sb.pending_empty_vals = Some(attr_values);
                                handled_tag = true;
                            } else {
                                if sb.pending_empty_pos == Some(pos as u64) {
                                    sb.pending_empty_pos = None;
                                    sb.pending_empty_vals = None;
                                }
                                open_structs.push(OpenStruct { name, start: pos as u64, attr_values });
                                handled_tag = true;
                            }
                        }
                    }
                }
                Tag::End { name } => {
                    if structs.contains_key(&name) {
                        if let Some(sb) = structs.get_mut(&name) {
                            if let Some(pend_pos) = sb.pending_empty_pos {
                                if pend_pos != pos as u64 {
                                    flush_pending_empty(sb)?;
                                }
                            }
                        }
                            let open = match open_structs.pop() {
                                Some(v) => v,
                                None => {
                                    err_closing_str.emit(
                                        lineno,
                                        &format!("closing non opened structure ({})", name),
                                    );
                                    // keep stack as-is
                                    // and ignore this end tag
                                    continue;
                                }
                            };
                            if open.name != name {
                                err_mismatch_str.emit(
                                    lineno,
                                    &format!("mismatched closing structure ({})", name),
                                );
                                open_structs.push(open);
                                handled_tag = true;
                            } else {
                            let sb = structs.get_mut(&name).unwrap();
                            if let Some(pend_pos) = sb.pending_empty_pos {
                                if pend_pos != pos as u64 {
                                    flush_pending_empty(sb)?;
                                }
                            }
                            let beg = open.start;
                            let end = pos as u64;
                            if sb.type64 {
                                sb.rng.write_all(&beg.to_le_bytes())?;
                                sb.rng.write_all(&end.to_le_bytes())?;
                            } else {
                                sb.rng.write_all(&(beg as u32).to_le_bytes())?;
                                sb.rng.write_all(&(end as u32).to_le_bytes())?;
                            }
                            let struct_pos = sb.count;
                            sb.count = sb.count.checked_add(1).ok_or("structure count overflow")?;
                            for (attr, val) in sb.attrs.iter_mut().zip(open.attr_values.iter()) {
                                let id = attr.id_for(val)?;
                                attr.push_value(id, struct_pos)?;
                            }
                            handled_tag = true;
                        }
                    }
                }
            }
        }
        if handled_tag {
            continue;
        }

        let fields: Vec<&str> = line.split('\t').collect();
        for (i, attr) in attrs.iter_mut().enumerate() {
            if i < fields.len() {
                attr.push_value(fields[i], pos)?;
            } else {
                let dv = attr.default_value.clone();
                attr.push_value(&dv, pos)?;
            }
        }
        flush_all_pending_at_pos(&mut structs, pos as u64)?;
        pos += 1;
    }

    if !open_structs.is_empty() {
        err_unterminated.emit(
            lineno,
            &format!("{} unterminated structure tags ignored", open_structs.len()),
        );
    }
    flush_all_pending_at_pos(&mut structs, pos as u64)?;

    for attr in attrs {
        attr.lex.finalize()?;
        attr.text.finalize()?;
    }

    for sb in structs.values_mut() {
        sb.rng.flush()?;
        for attr in sb.attrs.drain(..) {
            attr.lex.finalize()?;
            Box::new(attr.text).finalize()?;
        }
    }
    err_open_same_str.summary();
    err_closing_str.summary();
    err_mismatch_str.summary();
    err_unterminated.summary();

    Ok(())
}

fn flush_pending_empty(sb: &mut StructWriter) -> Result<(), Box<dyn std::error::Error>> {
    let pos = match sb.pending_empty_pos.take() {
        Some(p) => p,
        None => return Ok(()),
    };
    let vals = match sb.pending_empty_vals.take() {
        Some(v) => v,
        None => return Ok(()),
    };
    if sb.type64 {
        sb.rng.write_all(&pos.to_le_bytes())?;
        sb.rng.write_all(&pos.to_le_bytes())?;
    } else {
        sb.rng.write_all(&(pos as u32).to_le_bytes())?;
        sb.rng.write_all(&(pos as u32).to_le_bytes())?;
    }
    let struct_pos = sb.count;
    sb.count = sb.count.checked_add(1).ok_or("structure count overflow")?;
    for (attr, val) in sb.attrs.iter_mut().zip(vals.iter()) {
        let id = attr.id_for(val)?;
        attr.push_value(id, struct_pos)?;
    }
    Ok(())
}

fn flush_all_pending_at_pos(
    structs: &mut HashMap<String, StructWriter>,
    pos: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    for sb in structs.values_mut() {
        if sb.pending_empty_pos == Some(pos) {
            flush_pending_empty(sb)?;
        }
    }
    Ok(())
}
