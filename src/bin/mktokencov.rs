use corp::corp::{Corpus, CorpusLike};
use corp::subcorp::{SubCorpus, resolve_subc_path, struct_index_at_or_after_pos};

struct Args {
    corpname: String,
    structname: Option<String>,
    attrname: Option<String>,
    output_base: Option<String>,
    subcorpus: Option<String>,
}

fn usage(prog: &str) -> String {
    format!(
        "usage: {} CORPUS [STRUCTURE [ATTRIBUTE]] [-o BASE] [-s SUBCPATH]\ncount corpus positions covered by structure attribute values",
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

fn intersect_size(abeg: u64, aend: u64, bbeg: u64, bend: u64) -> u64 {
    aend.min(bend).saturating_sub(abeg.max(bbeg))
}

fn write_tokenfile(
    corpus: &dyn CorpusLike,
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

    let mut attrs = Vec::with_capacity(attrnames.len());
    for attrname in attrnames {
        let fullname = format!("{}.{}", structname, attrname);
        attrs.push(corpus.open_attribute(&fullname)?);
    }

    let mut tokencov = attrs
        .iter()
        .map(|attr| vec![0u64; attr.id_range() as usize])
        .collect::<Vec<_>>();

    let struct_len = structure.len() as u64;
    for attr in &attrs {
        if (attr.text().size() as u64) < struct_len {
            return Err("structure attribute lengths do not match".into());
        }
    }

    let mut add_coverage = |structpos: u64, len: u64| {
        if len == 0 {
            return;
        }
        for (idx, attr) in attrs.iter().enumerate() {
            let attr_id = attr.text().get(structpos) as usize;
            tokencov[idx][attr_id] += len;
        }
    };

    eprintln!("calculating token coverage for {}", structname);
    if let Some(ranges) = corpus.subcorp() {
        for &(rbeg, rend) in ranges.pairs() {
            if rbeg >= rend {
                continue;
            }
            let Some(mut structpos) = struct_index_at_or_after_pos(structure.as_ref(), rbeg) else {
                break;
            };
            while structpos < struct_len {
                let beg = structure.beg_at(structpos);
                if beg >= rend {
                    break;
                }
                let end = structure.end_at(structpos);
                let len = intersect_size(beg, end, rbeg, rend);
                add_coverage(structpos, len);
                structpos += 1;
            }
        }
    } else {
        for structpos in 0..struct_len {
            let beg = structure.beg_at(structpos);
            let end = structure.end_at(structpos);
            add_coverage(structpos, end.saturating_sub(beg));
        }
    }

    // TODO: MULTIVALUE/MULTISEP post-processing from mktokencov.cc is not supported yet.

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

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let fullcorp = Box::new(Corpus::open(&args.corpname)?);
    let subcorp = match &args.subcorpus {
        Some(subcpath) => {
            let resolved = resolve_subc_path(fullcorp.as_ref(), subcpath);
            Some(SubCorpus::from_corpus(fullcorp.as_ref(), &resolved)?)
        }
        None => None,
    };
    let corpus: &dyn CorpusLike = match subcorp.as_ref() {
        Some(sc) => sc as &dyn CorpusLike,
        None => fullcorp.as_ref() as &dyn CorpusLike,
    };

    let base = match (&args.output_base, &args.subcorpus) {
        (Some(base), _) => base.clone(),
        (None, Some(subcpath)) => {
            let resolved = resolve_subc_path(fullcorp.as_ref(), subcpath);
            resolved
                .strip_suffix(".subc")
                .unwrap_or(&resolved)
                .to_string()
                + "."
        }
        (None, None) => fullcorp.path.clone() + "/",
    };
    eprintln!("output prefix is {}", base);

    match (&args.structname, &args.attrname) {
        (Some(structname), Some(attrname)) => {
            write_tokenfile(corpus, structname, &[attrname], &base)?;
        }
        (Some(structname), None) => {
            let structure = fullcorp
                .conf
                .structure(structname)
                .ok_or_else(|| format!("unknown structure: {structname}"))?;
            let attrnames = structure.attrnames_in_order();
            write_tokenfile(corpus, structname, &attrnames, &base)?;
        }
        (None, None) => {
            for structname in fullcorp.conf.structnames_in_order() {
                let structure = fullcorp.conf.structure(structname).unwrap();
                let attrnames = structure.attrnames_in_order();
                write_tokenfile(corpus, &structname, &attrnames, &base)?;
            }
        }
        (None, Some(_)) => {
            return Err("ATTRIBUTE requires STRUCTURE".into());
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|msg| {
        eprintln!("{}", msg);
        "invalid command-line arguments"
    })?;
    run(args)
}
