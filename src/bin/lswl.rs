use std::cmp::Reverse;
use std::collections::BinaryHeap;

use corp::corp::CorpusLike;
use corp::subcorp::with_corpuslike_spec;

#[derive(Debug)]
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
    let mut it = std::env::args();
    let prog = it.next().unwrap_or_else(|| "lswl".to_string());

    let mut limit = 100usize;
    let mut minfreq = 5u64;
    let mut maxfreq = 0u64;
    let mut sorttype = "frq".to_string();
    let mut pos = Vec::<String>::new();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(usage(&prog)),
            "-l" => {
                let raw = it
                    .next()
                    .ok_or_else(|| format!("missing value for -l\n{}", usage(&prog)))?;
                let parsed = parse_u64(&raw, "-l", &prog)?;
                limit = usize::try_from(parsed)
                    .map_err(|_| format!("value for -l too large: {raw}\n{}", usage(&prog)))?;
            }
            "-i" => {
                let raw = it
                    .next()
                    .ok_or_else(|| format!("missing value for -i\n{}", usage(&prog)))?;
                minfreq = parse_u64(&raw, "-i", &prog)?.max(1);
            }
            "-a" => {
                let raw = it
                    .next()
                    .ok_or_else(|| format!("missing value for -a\n{}", usage(&prog)))?;
                maxfreq = parse_u64(&raw, "-a", &prog)?;
            }
            "-s" => {
                sorttype = it
                    .next()
                    .ok_or_else(|| format!("missing value for -s\n{}", usage(&prog)))?;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option {arg}\n{}", usage(&prog)));
            }
            _ => pos.push(arg),
        }
    }

    if pos.len() != 2 {
        return Err(usage(&prog));
    }

    Ok(Args {
        corpus: pos.remove(0),
        attr: pos.remove(0),
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
        keep_top(
            &mut heap,
            id,
            freq,
            args.limit,
            args.minfreq,
            args.maxfreq,
        );
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
