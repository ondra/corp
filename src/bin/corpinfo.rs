use corp::corp::Corpus;

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
    let mut it = std::env::args();
    let prog = it.next().unwrap_or_else(|| "corpinfo".to_string());

    let mut show_path = false;
    let mut show_size = false;
    let mut show_version = false;
    let mut corpus = None;

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(usage(&prog)),
            "-p" => show_path = true,
            "-s" => show_size = true,
            "-v" => show_version = true,
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option {arg}\n{}", usage(&prog)));
            }
            _ => {
                if corpus.is_some() {
                    return Err(format!("unexpected argument: {arg}\n{}", usage(&prog)));
                }
                corpus = Some(arg);
            }
        }
    }

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
