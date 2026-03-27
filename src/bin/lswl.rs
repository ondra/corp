use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::OsString;

use corp::corp::CorpusLike;
use corp::subcorp::with_corpuslike_spec;
use pico_args::Arguments;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    corpus: String,
    attr: String,
    limit: usize,
    minfreq: u64,
    maxfreq: u64,
    sorttype: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HeapItem {
    freq: u64,
    id: u32,
}

fn usage(prog: &str) -> String {
    format!(
        "Usage: {prog} [OPTIONS] CORPUS[:SUBCORPUS] ATTR\n\
         OPTIONS:\n\
         -l N    number of items to return (default 100)\n\
         -i N    minimum frequency (default 5)\n\
         -a N    maximum frequency, 0 disables it (default 0)\n\
         -s TYPE sort frequency type (default frq)"
    )
}

fn parse_u64(value: &str, flag: &str, prog: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
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
        .unwrap_or("lswl")
        .to_string();
    let mut pargs = Arguments::from_vec(args.into_iter().skip(1).collect());

    if pargs.contains(["-h", "--help"]) {
        return Err(usage(&prog));
    }

    let limit = pargs
        .opt_value_from_str::<_, String>("-l")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .map(|raw| {
            let parsed = parse_u64(&raw, "-l", &prog)?;
            usize::try_from(parsed)
                .map_err(|_| format!("value for -l too large: {raw}\n{}", usage(&prog)))
        })
        .transpose()?
        .unwrap_or(100);
    let minfreq = pargs
        .opt_value_from_str::<_, String>("-i")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .map(|raw| parse_u64(&raw, "-i", &prog).map(|value| value.max(1)))
        .transpose()?
        .unwrap_or(5);
    let maxfreq = pargs
        .opt_value_from_str::<_, String>("-a")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .map(|raw| parse_u64(&raw, "-a", &prog))
        .transpose()?
        .unwrap_or(0);
    let sorttype = pargs
        .opt_value_from_str::<_, String>("-s")
        .map_err(|e| format!("{e}\n{}", usage(&prog)))?
        .unwrap_or_else(|| "frq".to_string());

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
    if pos.len() != 2 {
        return Err(usage(&prog));
    }

    let mut pos = pos.into_iter();
    let corpus = pos
        .next()
        .expect("validated length")
        .into_string()
        .map_err(|_| usage(&prog))?;
    let attr = pos
        .next()
        .expect("validated length")
        .into_string()
        .map_err(|_| usage(&prog))?;

    Ok(Args {
        corpus,
        attr,
        limit,
        minfreq,
        maxfreq,
        sorttype,
    })
}

fn keep_top(
    heap: &mut BinaryHeap<Reverse<HeapItem>>,
    id: u32,
    freq: u64,
    limit: usize,
    minfreq: u64,
    maxfreq: u64,
) {
    if freq < minfreq {
        return;
    }
    if maxfreq != 0 && freq > maxfreq {
        return;
    }
    if limit == 0 {
        return;
    }

    let item = Reverse(HeapItem { freq, id });
    if heap.len() < limit {
        heap.push(item);
        return;
    }

    if let Some(smallest) = heap.peek() {
        if smallest.0.freq < freq {
            heap.pop();
            heap.push(item);
        }
    }
}

fn run_with_corpus(corpus: &dyn CorpusLike, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let attr = corpus.open_attribute(&args.attr)?;
    let sortfreq = attr.get_freq(&args.sorttype)?;

    let mut heap = BinaryHeap::<Reverse<HeapItem>>::new();
    for id in 0..attr.id_range() {
        let freq = sortfreq.frq(id);
        keep_top(&mut heap, id, freq, args.limit, args.minfreq, args.maxfreq);
    }

    let mut out = Vec::<HeapItem>::with_capacity(heap.len());
    while let Some(item) = heap.pop() {
        out.push(item.0);
    }
    out.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| a.id.cmp(&b.id)));

    for item in out {
        println!("{}\t{}", attr.id2str(item.id), item.freq);
    }

    Ok(())
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    with_corpuslike_spec(&args.corpus, |corpus| run_with_corpus(corpus, &args))
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
        let args = parse_vec(&["lswl", "-l100", "-i1", "-a500", "-sdocf", "corp", "word"])
            .expect("must parse");
        assert_eq!(
            args,
            Args {
                corpus: "corp".to_string(),
                attr: "word".to_string(),
                limit: 100,
                minfreq: 1,
                maxfreq: 500,
                sorttype: "docf".to_string(),
            }
        );
    }
}
