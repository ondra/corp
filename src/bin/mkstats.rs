use std::ffi::OsString;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use fs_err::File;
use pico_args::Arguments;

use corp::corp::{Corpus, CorpusLike, open_freq};
use corp::subcorp::{SubCorpus, resolve_subc_path};

#[derive(Debug, PartialEq, Eq)]
struct Args {
    force: bool,
    corpus: String,
    attr: String,
    stat: String,
    subcorpus: Option<String>,
}

fn usage(prog: &str) -> String {
    format!(
        "Usage: {prog} [-f] CORPNAME ATTR STAT [SUBCORP_FILE.subc]\n\
         {prog} [-f] CORPNAME:SUBCORP_FILE.subc ATTR STAT"
    )
}

fn split_corpus_spec(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        Some((corp, subc)) => (corp.to_string(), Some(subc.to_string())),
        None => (spec.to_string(), None),
    }
}

fn parse_args() -> Result<Args, String> {
    parse_args_from(std::env::args_os())
}

fn parse_args_from<I>(args: I) -> Result<Args, String>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let prog = args
        .first()
        .and_then(|arg| arg.to_str())
        .unwrap_or("mkstats")
        .to_string();
    let mut pargs = Arguments::from_vec(args.into_iter().skip(1).collect());

    if pargs.contains(["-h", "--help"]) {
        return Err(usage(&prog));
    }
    let force = pargs.contains("-f");
    let positional = pargs.finish();
    if let Some(arg) = positional
        .iter()
        .find(|arg| arg.to_string_lossy().starts_with('-'))
    {
        return Err(format!(
            "unknown option {}\n{}",
            arg.to_string_lossy(),
            usage(&prog)
        ));
    }
    let positional = positional
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| usage(&prog)))
        .collect::<Result<Vec<_>, _>>()?;

    if positional.len() != 3 && positional.len() != 4 {
        return Err(usage(&prog));
    }

    let mut positional = positional.into_iter();
    let corpus_spec = positional.next().expect("validated length");
    let attr = positional.next().expect("validated length");
    let stat = positional.next().expect("validated length");
    let positional_subc = positional.next();

    if stat != "frq" {
        return Err(format!(
            "unsupported STAT '{stat}', only 'frq' is supported\n{}",
            usage(&prog)
        ));
    }

    let (corpus, spec_subc) = split_corpus_spec(&corpus_spec);
    if corpus.is_empty() {
        return Err(format!(
            "missing corpus name in '{corpus_spec}'\n{}",
            usage(&prog)
        ));
    }

    if spec_subc.as_deref() == Some("") {
        return Err(format!(
            "missing subcorpus path in corpus spec '{corpus_spec}'\n{}",
            usage(&prog)
        ));
    }

    if spec_subc.is_some() && positional_subc.is_some() {
        return Err(format!(
            "subcorpus specified twice (in CORPNAME:SUBCORP and as positional argument)\n{}",
            usage(&prog)
        ));
    }

    Ok(Args {
        force,
        corpus,
        attr,
        stat,
        subcorpus: spec_subc.or(positional_subc),
    })
}

fn with_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut p = base.as_os_str().to_os_string();
    p.push(".");
    p.push(suffix);
    PathBuf::from(p)
}

fn subcorpus_freq_base(subcpath: &str, attr: &str) -> PathBuf {
    let base = subcpath.strip_suffix("subc").unwrap_or(subcpath);
    PathBuf::from(format!("{base}{attr}"))
}

fn write_freq_file(
    path: &Path,
    freqs: &[u64],
    use_u64: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp = with_suffix(path, "tmp");
    {
        let mut w = BufWriter::new(File::create(&tmp)?);
        if use_u64 {
            for &v in freqs {
                w.write_all(&v.to_le_bytes())?;
            }
        } else {
            for &v in freqs {
                let as_u32 = u32::try_from(v).map_err(|_| "frequency does not fit into u32")?;
                w.write_all(&as_u32.to_le_bytes())?;
            }
        }
        w.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn compute_and_write_frq(
    corpus: &dyn CorpusLike,
    attr_name: &str,
    out_base: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !force && open_freq(&out_base.to_string_lossy(), "frq").is_ok() {
        eprintln!("frq already compiled, skipping.");
        return Ok(());
    }

    let attr = corpus.open_attribute(attr_name)?;
    let id_range = attr.id_range() as usize;
    let mut freqs = vec![0u64; id_range];

    for id in attr.iter_ids(0) {
        let idx = id as usize;
        let slot = freqs
            .get_mut(idx)
            .ok_or_else(|| format!("token id {id} is outside id_range {id_range}"))?;
        *slot = slot.checked_add(1).ok_or("frequency overflow")?;
    }

    let max_freq = freqs.iter().copied().max().unwrap_or(0);
    let use_u64 = max_freq > u32::MAX as u64;
    let out_path = if use_u64 {
        with_suffix(out_base, "frq64")
    } else {
        with_suffix(out_base, "frq")
    };
    let stale_path = if use_u64 {
        with_suffix(out_base, "frq")
    } else {
        with_suffix(out_base, "frq64")
    };

    write_freq_file(&out_path, &freqs, use_u64)?;
    match std::fs::remove_file(stale_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let corpus = Corpus::open(&args.corpus)?;

    if let Some(subcpath) = args.subcorpus.as_deref() {
        let resolved = resolve_subc_path(&corpus, subcpath);
        let subcorp = SubCorpus::from_corpus(&corpus, &resolved)?;
        let out_base = subcorpus_freq_base(&resolved, &args.attr);
        compute_and_write_frq(&subcorp, &args.attr, &out_base, args.force)
    } else {
        let out_base = Path::new(&corpus.path).join(&args.attr);
        compute_and_write_frq(&corpus, &args.attr, &out_base, args.force)
    }
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
    fn parse_basic() {
        let args = parse_vec(&["mkstats", "corp", "word", "frq"]).expect("must parse");
        assert_eq!(
            args,
            Args {
                force: false,
                corpus: "corp".to_string(),
                attr: "word".to_string(),
                stat: "frq".to_string(),
                subcorpus: None,
            }
        );
    }

    #[test]
    fn parse_force_with_positional_subc() {
        let args =
            parse_vec(&["mkstats", "-f", "corp", "word", "frq", "sub/2.subc"]).expect("must parse");
        assert_eq!(args.force, true);
        assert_eq!(args.corpus, "corp");
        assert_eq!(args.subcorpus.as_deref(), Some("sub/2.subc"));
    }

    #[test]
    fn parse_subc_in_corpus_spec() {
        let args = parse_vec(&["mkstats", "corp:sub/2.subc", "word", "frq"]).expect("must parse");
        assert_eq!(args.corpus, "corp");
        assert_eq!(args.subcorpus.as_deref(), Some("sub/2.subc"));
    }

    #[test]
    fn reject_non_frq_stat() {
        let err = parse_vec(&["mkstats", "corp", "word", "arf"]).expect_err("must fail");
        assert!(err.contains("only 'frq' is supported"));
    }

    #[test]
    fn reject_double_subcorpus_specification() {
        let err = parse_vec(&["mkstats", "corp:sub/a.subc", "word", "frq", "sub/b.subc"])
            .expect_err("must fail");
        assert!(err.contains("subcorpus specified twice"));
    }

    #[test]
    fn reject_missing_subcorpus_after_colon() {
        let err = parse_vec(&["mkstats", "corp:", "word", "frq"]).expect_err("must fail");
        assert!(err.contains("missing subcorpus path"));
    }

    #[test]
    fn subcorpus_base_matches_runtime_rule() {
        let p = subcorpus_freq_base("/tmp/subcorp/2.subc", "word");
        assert_eq!(p.to_string_lossy(), "/tmp/subcorp/2.word");

        let p2 = subcorpus_freq_base("/tmp/subcorp/custom_subc", "lemma");
        assert_eq!(p2.to_string_lossy(), "/tmp/subcorp/custom_lemma");
    }
}
