use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use corp::corp::Corpus;
use pico_args::Arguments;
use regex::Regex;

use corp::query;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    corpus: String,
    subcorp_dir: String,
    stats: Option<String>,
    freqlistattrs: Option<String>,
    mode: Mode,
}

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    FromDefFile {
        def_file: String,
    },
    Direct {
        name: String,
        structure: String,
        spec: String,
    },
}

#[derive(Debug, Clone)]
struct Job {
    name: String,
    structure: String,
    spec: String,
}

#[derive(Debug)]
struct DefFile {
    freqlistattrs: Option<String>,
    records: Vec<Job>,
}

#[derive(Debug)]
struct CompiledTerm {
    attr_slot: usize,
    ids: HashSet<u32>,
}

fn usage(prog: &str) -> String {
    format!(
        "Usage:\n  {prog} [-s STATS] [-f FREQLISTATTRS] CORPUS SUBCORP_DIR SUBCORP_DEF_FILE\n  {prog} [-s STATS] [-f FREQLISTATTRS] CORPUS SUBCORP_DIR -n SUBCORP_NAME SUBCORP_STRUCT SUBCORP_STRUCTATTR_SPEC"
    )
}

fn parse_args() -> Result<Args, String> {
    parse_args_from(std::env::args_os())
}

fn parse_args_from<I, S>(args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let prog = args
        .first()
        .and_then(|arg| arg.to_str())
        .unwrap_or("mksubc")
        .to_string();
    let mut pargs = Arguments::from_vec(args.into_iter().skip(1).collect());

    if pargs.contains(["-h", "--help"]) {
        return Err(usage(&prog));
    }

    let stats = pargs
        .opt_value_from_str::<_, String>("-s")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?;
    let freqlistattrs = pargs
        .opt_value_from_str::<_, String>("-f")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?;
    let name = pargs
        .opt_value_from_str::<_, String>("-n")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?;
    let pos = pargs.finish();
    if let Some(arg) = pos
        .iter()
        .find(|arg| arg.to_string_lossy().starts_with('-'))
    {
        return Err(format!(
            "unknown option {}\n{}",
            arg.to_string_lossy(),
            usage(&prog)
        ));
    }
    let pos = pos
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| usage(&prog)))
        .collect::<Result<Vec<_>, _>>()?;

    let (corpus, subcorp_dir, mode) = if let Some(name) = name {
        if pos.len() != 4 {
            return Err(usage(&prog));
        }
        (
            pos[0].clone(),
            pos[1].clone(),
            Mode::Direct {
                name,
                structure: pos[2].clone(),
                spec: pos[3].clone(),
            },
        )
    } else {
        if pos.len() != 3 {
            return Err(usage(&prog));
        }
        (
            pos[0].clone(),
            pos[1].clone(),
            Mode::FromDefFile {
                def_file: pos[2].clone(),
            },
        )
    };

    Ok(Args {
        corpus,
        subcorp_dir,
        stats,
        freqlistattrs,
        mode,
    })
}

fn next_meaningful_line<'a>(lines: &'a [&'a str], idx: &mut usize) -> Option<(usize, &'a str)> {
    while *idx < lines.len() {
        let ln = *idx + 1;
        let line = lines[*idx].trim();
        *idx += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        return Some((ln, line));
    }
    None
}

fn parse_def_content(content: &str) -> Result<DefFile, String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0usize;
    let mut freqlistattrs = None;
    let mut records = Vec::<Job>::new();

    while let Some((line_no, line)) = next_meaningful_line(&lines, &mut i) {
        if let Some(rest) = line.strip_prefix("*FREQLISTATTRS") {
            freqlistattrs = Some(rest.trim().to_string());
            continue;
        }

        if let Some(rest) = line.strip_prefix('=') {
            let name = rest.trim();
            if name.is_empty() {
                return Err(format!(
                    "line {}: missing subcorpus name after '='",
                    line_no
                ));
            }

            let (struct_line_no, structure) =
                next_meaningful_line(&lines, &mut i).ok_or_else(|| {
                    format!(
                        "line {}: missing structure line for subcorpus '{}'",
                        line_no, name
                    )
                })?;
            if structure.starts_with('=') || structure.starts_with('*') {
                return Err(format!(
                    "line {}: expected structure name for subcorpus '{}'",
                    struct_line_no, name
                ));
            }

            let (spec_line_no, spec) = next_meaningful_line(&lines, &mut i).ok_or_else(|| {
                format!(
                    "line {}: missing query specification for subcorpus '{}'",
                    line_no, name
                )
            })?;
            if spec.starts_with('=') || spec.starts_with('*') {
                return Err(format!(
                    "line {}: expected query specification for subcorpus '{}'",
                    spec_line_no, name
                ));
            }

            records.push(Job {
                name: name.to_string(),
                structure: structure.to_string(),
                spec: spec.to_string(),
            });
            continue;
        }

        return Err(format!("line {}: unexpected content '{}'", line_no, line));
    }

    Ok(DefFile {
        freqlistattrs,
        records,
    })
}

fn parse_def_file(path: &str) -> Result<DefFile, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    parse_def_content(&content).map_err(Into::into)
}

fn merge_ranges(mut ranges: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    ranges.retain(|(beg, end)| beg < end);
    ranges.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut merged = Vec::<(u64, u64)>::new();
    for (beg, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut() {
            if beg <= *last_end {
                if end > *last_end {
                    *last_end = end;
                }
                continue;
            }
        }
        merged.push((beg, end));
    }
    merged
}

fn compile_term_ids(
    attr: &dyn corp::corp::Attr,
    pattern: &str,
) -> Result<HashSet<u32>, Box<dyn std::error::Error>> {
    let anchored = format!("^(?:{})$", pattern);
    let re = Regex::new(&anchored)?;
    let mut ids = HashSet::new();
    for id in 0..attr.id_range() {
        if re.is_match(attr.id2str(id)) {
            ids.insert(id);
        }
    }
    Ok(ids)
}

fn build_subcorpus_ranges(
    corpus: &Corpus,
    structure_name: &str,
    spec: &str,
) -> Result<Vec<(u64, u64)>, Box<dyn std::error::Error>> {
    let parsed =
        query::parse_spec(spec).map_err(|e| format!("invalid query specification: {e}"))?;
    let structure = corpus.open_struct(structure_name)?;
    let struct_len = structure.len() as u64;

    let mut attr_slots = HashMap::<String, usize>::new();
    let mut attrs = Vec::new();
    for term in &parsed.terms {
        if attr_slots.contains_key(&term.attr) {
            continue;
        }
        let full_attr = format!("{}.{}", structure_name, term.attr);
        let attr = corpus.open_attribute(&full_attr)?;
        if (attr.text().size() as u64) < struct_len {
            return Err(format!(
                "structure attribute '{}' has insufficient length for structure '{}'",
                full_attr, structure_name
            )
            .into());
        }
        let slot = attrs.len();
        attrs.push(attr);
        attr_slots.insert(term.attr.clone(), slot);
    }

    let mut compiled_terms = Vec::<CompiledTerm>::with_capacity(parsed.terms.len());
    for term in &parsed.terms {
        let attr_slot = *attr_slots
            .get(&term.attr)
            .ok_or_else(|| format!("internal error: missing attribute slot '{}'", term.attr))?;
        let ids = compile_term_ids(attrs[attr_slot].as_ref(), &term.pattern).map_err(|e| {
            format!(
                "failed to compile regex for '{}': {} ({e})",
                term.attr, term.pattern
            )
        })?;
        compiled_terms.push(CompiledTerm { attr_slot, ids });
    }

    let mut ranges = Vec::<(u64, u64)>::new();
    let mut curr_attr_ids = vec![0u32; attrs.len()];
    let mut term_truth = vec![false; compiled_terms.len()];

    for i in 0..struct_len {
        for (slot, attr) in attrs.iter().enumerate() {
            curr_attr_ids[slot] = attr.text().get(i);
        }
        for (ti, term) in compiled_terms.iter().enumerate() {
            term_truth[ti] = term.ids.contains(&curr_attr_ids[term.attr_slot]);
        }
        if query::eval_expr(&parsed.expr, &term_truth) {
            ranges.push((structure.beg_at(i), structure.end_at(i)));
        }
    }

    Ok(merge_ranges(ranges))
}

fn ensure_subc_filename(name: &str) -> String {
    if name.ends_with(".subc") {
        name.to_string()
    } else {
        format!("{name}.subc")
    }
}

fn write_subc(path: &Path, ranges: &[(u64, u64)]) -> Result<(), Box<dyn std::error::Error>> {
    let file = fs::File::create(path)?;
    let mut w = BufWriter::new(file);
    for &(beg, end) in ranges {
        w.write_all(&beg.to_le_bytes())?;
        w.write_all(&end.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stats) = args.stats.as_deref() {
        eprintln!("mksubc: STATS parsing is not implemented yet, value kept: {stats}");
    }
    if let Some(attrs) = args.freqlistattrs.as_deref() {
        eprintln!("mksubc: -f FREQLISTATTRS parsing is not implemented yet, value kept: {attrs}");
    }

    let jobs = match &args.mode {
        Mode::Direct {
            name,
            structure,
            spec,
        } => vec![Job {
            name: name.clone(),
            structure: structure.clone(),
            spec: spec.clone(),
        }],
        Mode::FromDefFile { def_file } => {
            let def = parse_def_file(def_file)?;
            if let Some(attrs) = def.freqlistattrs.as_deref() {
                eprintln!(
                    "mksubc: *FREQLISTATTRS in def-file parsing is not implemented yet, value kept: {attrs}"
                );
            }
            def.records
        }
    };

    fs::create_dir_all(&args.subcorp_dir)?;
    let out_dir = PathBuf::from(&args.subcorp_dir);

    let corpus = Corpus::open(&args.corpus)?;
    for job in jobs {
        eprintln!(
            "mksubc: compiling subcorpus '{}' using {} / {}",
            job.name, job.structure, job.spec
        );
        let ranges = build_subcorpus_ranges(&corpus, &job.structure, &job.spec)
            .map_err(|e| format!("failed to build subcorpus '{}': {e}", job.name))?;
        let out_path = out_dir.join(ensure_subc_filename(&job.name));
        write_subc(&out_path, &ranges)?;
        eprintln!("mksubc: wrote {}", out_path.display());
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|msg| {
        eprintln!("{msg}");
        "invalid command-line arguments"
    })?;
    run(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_vec(args: &[&str]) -> Result<Args, String> {
        parse_args_from(args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parse_def_content_records_and_comments() {
        let txt = r#"
# comment
*FREQLISTATTRS word lemma

=sub1
  doc
  id="x"

# another
=sub2
  file
  name="y" & kind="z"
"#;
        let def = parse_def_content(txt).expect("must parse");
        assert_eq!(def.freqlistattrs.as_deref(), Some("word lemma"));
        assert_eq!(def.records.len(), 2);
        assert_eq!(def.records[0].name, "sub1");
        assert_eq!(def.records[1].structure, "file");
    }

    #[test]
    fn merge_ranges_overlaps_and_adjacency() {
        let merged = merge_ranges(vec![(10, 20), (20, 30), (5, 8), (7, 9), (100, 100)]);
        assert_eq!(merged, vec![(5, 9), (10, 30)]);
    }

    #[test]
    fn parse_attached_short_values() {
        let args = parse_vec(&[
            "mksubc",
            "-sstats.txt",
            "-fword",
            "-nsub1",
            "corp",
            "out",
            "doc",
            "id=\"x\"",
        ])
        .expect("must parse");
        assert_eq!(
            args,
            Args {
                corpus: "corp".to_string(),
                subcorp_dir: "out".to_string(),
                stats: Some("stats.txt".to_string()),
                freqlistattrs: Some("word".to_string()),
                mode: Mode::Direct {
                    name: "sub1".to_string(),
                    structure: "doc".to_string(),
                    spec: "id=\"x\"".to_string(),
                },
            }
        );
    }
}
