use std::fmt;

use nom::{
  IResult, Finish,
  error::{ParseError, VerboseError, convert_error},
  bytes::complete::{tag, take_while, take_while1, is_a},
  combinator::{opt, all_consuming, map, recognize},
  branch::alt,
  multi::{fold_many0, fold_many1},
  sequence::{tuple, delimited, separated_pair},
};

type S = str;

#[derive(Debug)]
#[derive(Clone)]
pub enum Elem {
    Structure(String, Block),
    Attribute(String, Block),
    KV(String, String),
}

#[derive(Debug)]
#[derive(Clone)]
pub struct Block {
    elems: Vec<Elem>,
}

impl Block {
    pub fn structure(&self, name: &str) -> Option<&Block> {
        for e in &self.elems {
            match e {
                Elem::Structure(n, block) =>
                    if name == n { return Some(block) },
                _ => continue
            }
        }
        None
    }

    pub fn attribute(&self, name: &str) -> Option<&Block> {
        for e in &self.elems {
            match e {
                Elem::Attribute(n, block) =>
                    if name == n { return Some(block) },
                _ => continue
            }
        }
        None
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        for e in &self.elems {
            match e {
                Elem::KV(n, value) =>
                    if name == n { return Some(value) },
                _ => continue
            }
        }
        None
    }

    pub fn attrnames_in_order(&self) -> Vec<&str> {
        self.elems.iter().filter_map(|e| match e {
                Elem::Attribute(name, _) => Some(name.as_str()), _ => None })
            .collect()
    }

    pub fn structnames_in_order(&self) -> Vec<&str> {
        self.elems.iter().filter_map(|e| match e {
                Elem::Structure(name, _) => Some(name.as_str()), _ => None })
            .collect()
    }
}

fn indent(s: &str) -> String {
    let mut out = String::new();
    for part in s.lines() {
        if !part.is_empty() {
            out.push_str("    ");
            out.push_str(part);
            out.push('\n');
        } else {
            out.push('\n');
        }
    }
    out
}

impl fmt::Display for Elem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Elem::Structure(name, blk) => {
                f.write_str("STRUCTURE ")?; 
                f.write_str(name)?;
                if !blk.elems.is_empty() {
                    f.write_str(" {\n")?;
                    let s = format!("{}", blk);
                    f.write_str(&indent(&s))?;
                    f.write_str("}")
                } else { Ok(()) }
            },
            Elem::Attribute(name, blk) => { 
                f.write_str("ATTRIBUTE ")?; 
                f.write_str(name)?;
                if !blk.elems.is_empty() {
                    f.write_str(" {\n")?;
                    let s = format!("{}", blk);
                    f.write_str(&indent(&s))?;
                    f.write_str("}")
                } else { Ok(()) }
            },
            Elem::KV(k, v) => { write!(f, "{} \"{}\"", k, v) },
        }?;
        Ok(())
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.elems {
            writeln!(f, "{}", e)?;
        }
        Ok(())
    }
}


fn parse_ws<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, &'a S, E> {
    take_while(|c: char| {" \t".contains(c)})(inp)
}

fn parse_nl<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, &'a S, E> {
    recognize(tuple((
        parse_ws,
        opt(
            tuple((tag("#"), take_while(|c: char| c != '\n')))
        ),
        is_a("\n"),
        parse_ws
    ))) (inp)
}

fn parse_str<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, &'a S, E> {
    alt((
        delimited(tag("\""), take_while(|c: char| c != '"'), tag("\"")),
        delimited(tag("'"),  take_while(|c: char| c != '\''),  tag("'")),
        take_while1(|c: char| {c != ' ' && c != '#' && c != '\t' && c != '\n'}
    )))(inp)
}

fn parse_name<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, &'a S, E> {
    take_while1(|c: char| {c.is_ascii_alphanumeric()})(inp)
}

fn parse_kv<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, Elem, E> {
    map(
        separated_pair(parse_name, parse_ws, parse_str),
        |(k, v)| Elem::KV(k.to_string(), v.to_string())
    )(inp)
}

fn parse_attribute<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, Elem, E> {
    map(
        tuple((
            delimited(
                tuple((tag("ATTRIBUTE"), parse_ws)),
                parse_str,
                parse_ws),
            opt(
                delimited(
                    tuple((tag("{"), parse_manynl)),
                    parse_blk,
                    tag("}")
                )
            )
        )),
        |(name, block)| {
            Elem::Attribute(
                name.to_string(), 
                block.unwrap_or(Block{ elems: Vec::<Elem>::new()})
            )
        }
    )(inp)
}

fn parse_structure<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, Elem, E> {
    map(
        tuple((
            delimited(
                tuple((tag("STRUCTURE"), parse_ws)),
                parse_str,
                parse_ws
            ),
            opt(
                delimited(
                    tuple((tag("{"), parse_manynl)),
                    parse_blk,
                    tag("}")
                )
            )
        )),
        |(name, maybe_block)| {
            Elem::Structure(
                name.to_string(), 
                maybe_block.unwrap_or(Block{ elems: Vec::<Elem>::new()})
            )
        }
    )(inp)
}

fn parse_manynl<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, (), E> {
    fold_many1(
        parse_nl,
        || {()},
        |_, _item| {})(inp)
}

fn parse_elem<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, Elem, E> {
    delimited(
        opt(tuple((parse_ws, parse_manynl))),
        alt((
            parse_attribute,
            parse_structure,
            parse_kv,
        )),
        parse_manynl
    )(inp)
}

fn parse_blk<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, Block, E> {
    map(fold_many0(
        parse_elem,
        || {Vec::new()},
        |mut vec: Vec<Elem>, item| {
            vec.push(item);
            vec
        }
        ), |x| Block{elems:x} 
    )(inp)
}

fn parse_conf<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, Block, E> {
    all_consuming(parse_blk)(inp)
}

#[derive(Debug)]
pub struct ConfError {
    parser_err: String,
}

impl fmt::Display for ConfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.parser_err)
    }
}

impl std::error::Error for ConfError {}

pub fn parse_conf_opt(inp: &S) -> Result<Block, ConfError> {
    match parse_conf::<VerboseError<&S>>(inp).finish() {
        Ok((_rest, block)) => Ok(block),
        Err(e) => {
            //println!("ERROR =======\n{}", convert_error(inp, e));
            Err(ConfError{ parser_err: convert_error(inp, e) })
        }
    }
}

