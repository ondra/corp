struct Args {
    corpname: String,
    structname: Option<String>,
    attrname: Option<String>,
    output_base: Option<String>,
    subcorpus: Option<String>,
}

fn usage(prog: &str) -> String {
    format!(
        "usage: {} CORPUS [STRUCTURE [ATTRIBUTE]] [-o BASE]\ncount corpus positions covered by structure attribute values",
        prog
    )
}

fn parse_args() -> Result<Args, String> {
    let mut iter = std::env::args();
    let prog = iter.next().unwrap_or_else(|| "mktokencov".to_string());

    let mut output_base = None;
    let mut subcorpus = None;
    let mut positionals = Vec::<String>::new();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Err(usage(&prog));
            }
            "-o" | "--output-base" => {
                output_base = Some(
                    iter.next()
                        .ok_or_else(|| format!("missing value for {}\n{}", arg, usage(&prog)))?,
                );
            }
            "-s" | "--subcorpus" => {
                subcorpus = Some(
                    iter.next()
                        .ok_or_else(|| format!("missing value for {}\n{}", arg, usage(&prog)))?,
                );
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option {}\n{}", arg, usage(&prog)));
            }
            _ => positionals.push(arg),
        }
    }

    let args = match positionals.len() {
        1 => Args {
            corpname: positionals[0].clone(),
            structname: None,
            attrname: None,
            output_base,
            subcorpus,
        },
        2 => Args {
            corpname: positionals[0].clone(),
            structname: Some(positionals[1].clone()),
            attrname: None,
            output_base,
            subcorpus,
        },
        3 => Args {
            corpname: positionals[0].clone(),
            structname: Some(positionals[1].clone()),
            attrname: Some(positionals[2].clone()),
            output_base,
            subcorpus,
        },
        _ => return Err(usage(&prog)),
    };

    Ok(args)
}

fn write_tokenfile(
    corpus: &corp::corp::Corpus,
    structname: &str,
    attrnames: &[&str],
    base: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if attrnames.is_empty() {
        eprintln!(
            "skipping token coverage calculation for structure {} without attributes",
            structname
        );
        return Ok(());
    }

    let structure = corpus.open_struct(structname)?;

    let mut attrs = Vec::<Box<dyn corp::corp::Attr>>::with_capacity(attrnames.len());
    for attrname in attrnames {
        let fullname = format!("{}.{}", structname, attrname);
        attrs.push(corpus.open_attribute(&fullname)?);
    }

    let mut iters = attrs
        .iter()
        .map(|attr| attr.iter_ids(0))
        .collect::<Vec<_>>();
    let mut tokencov = attrs
        .iter()
        .map(|attr| vec![0u64; attr.id_range() as usize])
        .collect::<Vec<_>>();

    eprintln!("calculating token coverage for {}", structname);
    let mut structpos = 0u64;
    while let Some(first_attr_id) = iters[0].next() {
        let beg = structure.beg_at(structpos);
        let end = structure.end_at(structpos);
        let len = end.saturating_sub(beg) as u64;
        tokencov[0][first_attr_id as usize] += len;

        for idx in 1..iters.len() {
            let attr_id = iters[idx]
                .next()
                .ok_or("structure attribute lengths do not match")?;
            tokencov[idx][attr_id as usize] += len;
        }
        structpos += 1;
    }

    for idx in 1..iters.len() {
        if iters[idx].next().is_some() {
            return Err("structure attribute lengths do not match".into());
        }
    }

    for (idx, attrname) in attrnames.iter().enumerate() {
        let fpath = format!("{}{}.{}.token", base, structname, attrname);
        eprintln!("writing {}", fpath);
        let mut writer = std::io::BufWriter::new(std::fs::File::create(&fpath)?);
        use std::io::Write;
        for cov in &tokencov[idx] {
            writer.write_all(&cov.to_ne_bytes())?;
        }
        writer.flush()?;
    }

    eprintln!("finished writing token coverage for {}", structname);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|msg| {
        eprintln!("{}", msg);
        "invalid command-line arguments"
    })?;

    if args.subcorpus.is_some() {
        return Err("subcorpora are not supported by this Rust port".into());
    }

    let corpus = corp::corp::Corpus::open(&args.corpname)?;
    let base = args.output_base.unwrap_or_else(|| corpus.path.clone() + "/");
    eprintln!("output prefix is {}", base);

    match (&args.structname, &args.attrname) {
        (Some(structname), Some(attrname)) => {
            write_tokenfile(&corpus, structname, &[attrname], &base)?;
        }
        (Some(structname), None) => {
            let attrnames = corpus.conf.attrnames_in_order();
            write_tokenfile(&corpus, structname, &attrnames, &base)?;
        }
        (None, None) => {
            for structname in corpus.conf.structnames_in_order() {
                let structure = corpus.conf.structure(structname).unwrap();
                let attrnames = structure.attrnames_in_order();
                write_tokenfile(&corpus, &structname, &attrnames, &base)?;
            }
        }
        (None, Some(_)) => {
            return Err("ATTRIBUTE requires STRUCTURE".into());
        }
    }

    Ok(())
}
