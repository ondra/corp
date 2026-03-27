use std::ffi::OsString;

use corp::corp::{Attr, CorpusLike};
use corp::structure::Struct;
use corp::subcorp::with_corpuslike_spec;
use pico_args::Arguments;
use rand::SeedableRng;
use rand::rngs::StdRng;

const VERSION: &str = git_version::git_version!(args = ["--tags", "--always", "--dirty"]);

#[derive(Debug, PartialEq, Eq)]
struct Args {
    corpus: Option<String>,
    word: Option<String>,
    attr: Option<String>,
    query_attr: Option<String>,
    sample: Option<usize>,
    limit: Option<usize>,
    window: usize,
    tab: bool,
    glue: Option<String>,
    version: bool,
}

fn usage(prog: &str) -> String {
    format!(
        "Usage: {prog} [OPTIONS] CORPUS[:SUBCORPUS] WORD\n\
         OPTIONS:\n\
         -a ATTR  attribute to display (default: DEFAULTATTR or word)\n\
         -q ATTR  attribute to query (default: same as -a)\n\
         -n N     sample at most N concordance lines\n\
         -l N     output at most N lines\n\
         -w N     context window in tokens per side (default 25)\n\
         -t       tab-separated output (left\\tkeyword\\tright)\n\
         -g STRUCT enable glue processing with named structure\n\
         -v       print version\n\
         -h       print this help"
    )
}

fn parse_usize(value: &str, flag: &str, prog: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("invalid value for {flag}: {value}\n{}", usage(prog)))
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
        .unwrap_or("conc")
        .to_string();
    let mut pargs = Arguments::from_vec(args.into_iter().skip(1).collect());

    if pargs.contains(["-h", "--help"]) {
        return Err(usage(&prog));
    }

    let version = pargs.contains("-v");
    let tab = pargs.contains("-t");
    let attr = pargs
        .opt_value_from_str::<_, String>("-a")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?;
    let query_attr = pargs
        .opt_value_from_str::<_, String>("-q")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?;
    let sample = pargs
        .opt_value_from_str::<_, String>("-n")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .map(|raw| parse_usize(&raw, "-n", &prog))
        .transpose()?;
    let limit = pargs
        .opt_value_from_str::<_, String>("-l")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .map(|raw| parse_usize(&raw, "-l", &prog))
        .transpose()?;
    let window = pargs
        .opt_value_from_str::<_, String>("-w")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .map(|raw| parse_usize(&raw, "-w", &prog))
        .transpose()?
        .unwrap_or(25);
    let glue = pargs
        .opt_value_from_str::<_, String>("-g")
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
    let mut pos = pos
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| usage(&prog)))
        .collect::<Result<Vec<_>, _>>()?;

    if !version && pos.len() < 2 {
        return Err(usage(&prog));
    }

    let (corpus, word) = if pos.len() >= 2 {
        (Some(pos.remove(0)), Some(pos.remove(0)))
    } else {
        (pos.pop(), None)
    };

    Ok(Args {
        corpus,
        word,
        attr,
        query_attr,
        sample,
        limit,
        window,
        tab,
        glue,
        version,
    })
}

fn format_line(
    attr: &dyn Attr,
    pos: u64,
    window: usize,
    tab: bool,
    glue: Option<&(dyn Struct + Sync + Send)>,
    corpus_size: u64,
) -> String {
    let start = pos.saturating_sub(window as u64);
    let end = (pos + 1 + window as u64).min(corpus_size);
    let count = (end - start) as usize;

    let mut ids: Vec<u32> = Vec::with_capacity(count);
    let mut iter = attr.iter_ids(start);
    for _ in 0..count {
        if let Some(id) = iter.next() {
            ids.push(id);
        } else {
            break;
        }
    }

    let hit_offset = (pos - start) as usize;

    if tab {
        let mut left = Vec::new();
        let mut right = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            let w = attr.id2str(id);
            if i < hit_offset {
                left.push(w);
            } else if i > hit_offset {
                right.push(w);
            }
        }
        let kw = if hit_offset < ids.len() {
            attr.id2str(ids[hit_offset])
        } else {
            ""
        };
        format!("{}\t{}\t{}", left.join(" "), kw, right.join(" "))
    } else {
        let mut out = String::new();
        for (i, &id) in ids.iter().enumerate() {
            let w = attr.id2str(id);
            let token_pos = start + i as u64;

            if i > 0 {
                let need_space = if let Some(g) = glue {
                    g.find_beg(token_pos) != token_pos
                } else {
                    true
                };
                if need_space {
                    out.push(' ');
                }
            }

            if i == hit_offset {
                out.push('<');
                out.push_str(w);
                out.push('>');
            } else {
                out.push_str(w);
            }
        }
        out
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if args.version {
        println!("{VERSION}");
        if args.corpus.is_none() {
            return Ok(());
        }
    }

    let corpus_name = args.corpus.as_ref().ok_or("missing CORPUS argument")?;
    let word = args.word.as_ref().ok_or("missing WORD argument")?;

    with_corpuslike_spec(corpus_name, |corpus| run_with_corpus(corpus, &args, word))
}

fn run_with_corpus(
    corpus: &dyn CorpusLike,
    args: &Args,
    word: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let default_attr = || {
        corpus
            .get_conf("DEFAULTATTR")
            .unwrap_or_else(|| "word".to_string())
    };
    let display_attr_name = args.attr.clone().unwrap_or_else(&default_attr);
    let query_attr_name = args.query_attr.clone().unwrap_or_else(default_attr);

    let display_attr = corpus.open_attribute(&display_attr_name)?;
    let query_attr = if query_attr_name == display_attr_name {
        None
    } else {
        Some(corpus.open_attribute(&query_attr_name)?)
    };

    let qa = query_attr.as_deref().unwrap_or(display_attr.as_ref());
    let id = qa
        .str2id(word)
        .ok_or_else(|| format!("word not found: {word}"))?;

    let glue: Option<Box<dyn Struct + Sync + Send>> = if let Some(ref g) = args.glue {
        Some(corpus.open_struct(g)?)
    } else {
        None
    };

    let corpus_size = display_attr.text().size() as u64;
    let glue_ref = glue.as_deref();

    let mut rng = StdRng::seed_from_u64(666);
    let limit = args.limit.unwrap_or(usize::MAX);

    let poss_sampler = qa.id2poss_sampler(args.sample);
    for pos in poss_sampler.id2poss_with_rng(id, &mut rng).take(limit) {
        let line = format_line(
            display_attr.as_ref(),
            pos,
            args.window,
            args.tab,
            glue_ref,
            corpus_size,
        );
        println!("{pos}\t{line}");
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
    fn parse_attached_short_values() {
        let args = parse_vec(&[
            "conc", "-aword", "-qlemma", "-l10", "-w5", "-gdoc", "corp", "the",
        ])
        .expect("must parse");
        assert_eq!(
            args,
            Args {
                corpus: Some("corp".to_string()),
                word: Some("the".to_string()),
                attr: Some("word".to_string()),
                query_attr: Some("lemma".to_string()),
                sample: None,
                limit: Some(10),
                window: 5,
                tab: false,
                glue: Some("doc".to_string()),
                version: false,
            }
        );
    }
}
