use corp::corp::{Attr, CorpusLike};
use corp::structure::Struct;
use corp::subcorp::with_corpuslike_spec;
use rand::rngs::StdRng;
use rand::SeedableRng;

const VERSION: &str = git_version::git_version!(args = ["--tags", "--always", "--dirty"]);

#[derive(Debug)]
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
    let mut it = std::env::args();
    let prog = it.next().unwrap_or_else(|| "conc".to_string());

    let mut attr = None;
    let mut query_attr = None;
    let mut sample = None;
    let mut limit = None;
    let mut window = 25usize;
    let mut tab = false;
    let mut glue = None;
    let mut version = false;
    let mut pos = Vec::<String>::new();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(usage(&prog)),
            "-v" => version = true,
            "-t" => tab = true,
            "-a" => {
                attr = Some(
                    it.next()
                        .ok_or_else(|| format!("missing value for -a\n{}", usage(&prog)))?,
                );
            }
            "-q" => {
                query_attr = Some(
                    it.next()
                        .ok_or_else(|| format!("missing value for -q\n{}", usage(&prog)))?,
                );
            }
            "-n" => {
                let raw = it
                    .next()
                    .ok_or_else(|| format!("missing value for -n\n{}", usage(&prog)))?;
                sample = Some(parse_usize(&raw, "-n", &prog)?);
            }
            "-l" => {
                let raw = it
                    .next()
                    .ok_or_else(|| format!("missing value for -l\n{}", usage(&prog)))?;
                limit = Some(parse_usize(&raw, "-l", &prog)?);
            }
            "-w" => {
                let raw = it
                    .next()
                    .ok_or_else(|| format!("missing value for -w\n{}", usage(&prog)))?;
                window = parse_usize(&raw, "-w", &prog)?;
            }
            "-g" => {
                glue = Some(
                    it.next()
                        .ok_or_else(|| format!("missing value for -g\n{}", usage(&prog)))?,
                );
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option {arg}\n{}", usage(&prog)));
            }
            _ => pos.push(arg),
        }
    }

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
        let line = format_line(display_attr.as_ref(), pos, args.window, args.tab, glue_ref, corpus_size);
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
