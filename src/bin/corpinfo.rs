use std::ffi::OsString;

use corp::corp::Corpus;
use pico_args::Arguments;

const VERSION: &str = git_version::git_version!(args = ["--tags", "--always", "--dirty"]);

#[derive(Debug)]
struct Args {
    corpus: Option<String>,
    show_path: bool,
    show_size: bool,
    show_version: bool,
}

fn usage(prog: &str) -> String {
    format!(
        "Usage: {prog} [OPTIONS] CORPUS\n\
         OPTIONS:\n\
         -p      print corpus path\n\
         -s      print corpus size\n\
         -v      print version\n\
         -h      print this help"
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
        .unwrap_or("corpinfo")
        .to_string();
    let mut pargs = Arguments::from_vec(args.into_iter().skip(1).collect());

    if pargs.contains(["-h", "--help"]) {
        return Err(usage(&prog));
    }

    let show_path = pargs.contains("-p");
    let show_size = pargs.contains("-s");
    let show_version = pargs.contains("-v");
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
    if pos.len() > 1 {
        return Err(format!(
            "unexpected argument: {}\n{}",
            pos[1].to_string_lossy(),
            usage(&prog)
        ));
    }
    let corpus = pos
        .into_iter()
        .next()
        .map(|arg| arg.into_string().map_err(|_| usage(&prog)))
        .transpose()?;

    if !show_version && corpus.is_none() {
        return Err(usage(&prog));
    }

    Ok(Args {
        corpus,
        show_path,
        show_size,
        show_version,
    })
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if args.show_version {
        println!("{VERSION}");
        if args.corpus.is_none() {
            return Ok(());
        }
    }

    let corpus_name = args.corpus.as_ref().unwrap();
    let corpus = Corpus::open(corpus_name)?;

    if args.show_path {
        println!("{}", corpus.path);
    }

    if args.show_size {
        let default_attr = corpus
            .get_conf("DEFAULTATTR")
            .unwrap_or_else(|| "word".to_string());
        let attr = corpus.open_attribute(&default_attr)?;
        println!("{}", attr.text().size());
    }

    if !args.show_path && !args.show_size && !args.show_version {
        eprintln!("{}", usage("corpinfo"));
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
