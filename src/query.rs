use std::collections::HashMap;

use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while_m_n};
use nom::combinator::{all_consuming, map, recognize};
use nom::error::{convert_error, ParseError, VerboseError, VerboseErrorKind};
use nom::sequence::{delimited, preceded, tuple};
use nom::{Finish, IResult};

type S = str;

type VErr<'a> = VerboseError<&'a S>;

#[derive(Debug, Clone)]
pub enum Expr {
    Term(usize),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct TermSpec {
    pub attr: String,
    pub pattern: String,
}

#[derive(Debug)]
pub struct ParsedSpec {
    pub expr: Expr,
    pub terms: Vec<TermSpec>,
}

#[derive(Debug, Clone)]
enum RawExpr {
    Term(TermSpec),
    And(Box<RawExpr>, Box<RawExpr>),
    Or(Box<RawExpr>, Box<RawExpr>),
}

fn parse_ws<'a, E: ParseError<&'a S>>(inp: &'a S) -> IResult<&'a S, &'a S, E> {
    take_while(|c: char| c.is_whitespace())(inp)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

fn parse_ident(inp: &S) -> IResult<&S, String, VErr<'_>> {
    map(
        recognize(tuple((
            take_while_m_n(1, 1, is_ident_start),
            take_while(is_ident_char),
        ))),
        |s: &str| s.to_string(),
    )(inp)
}

fn err_ctx<'a>(inp: &'a S, ctx: &'static str) -> nom::Err<VErr<'a>> {
    nom::Err::Error(VErr {
        errors: vec![(inp, VerboseErrorKind::Context(ctx))],
    })
}

fn fail_ctx<'a>(inp: &'a S, ctx: &'static str) -> nom::Err<VErr<'a>> {
    nom::Err::Failure(VErr {
        errors: vec![(inp, VerboseErrorKind::Context(ctx))],
    })
}

fn parse_quoted(inp: &S) -> IResult<&S, String, VErr<'_>> {
    let mut chars = inp.chars();
    let Some(quote) = chars.next() else {
        return Err(err_ctx(inp, "expected quoted regex value"));
    };
    if quote != '"' && quote != '\'' {
        return Err(err_ctx(inp, "expected quoted regex value"));
    }

    let mut i = quote.len_utf8();
    let mut out = String::new();
    while i < inp.len() {
        let ch = inp[i..]
            .chars()
            .next()
            .ok_or_else(|| fail_ctx(&inp[i..], "invalid UTF-8 boundary"))?;

        if ch == quote {
            i += ch.len_utf8();
            return Ok((&inp[i..], out));
        }

        if ch == '\\' {
            i += ch.len_utf8();
            if i >= inp.len() {
                return Err(fail_ctx(&inp[i - 1..], "unterminated escape sequence"));
            }
            let esc = inp[i..]
                .chars()
                .next()
                .ok_or_else(|| fail_ctx(&inp[i..], "invalid UTF-8 boundary"))?;
            if esc == quote || esc == '\\' {
                out.push(esc);
            } else {
                out.push('\\');
                out.push(esc);
            }
            i += esc.len_utf8();
            continue;
        }

        out.push(ch);
        i += ch.len_utf8();
    }

    Err(fail_ctx(inp, "unterminated quoted string"))
}

fn parse_term(inp: &S) -> IResult<&S, RawExpr, VErr<'_>> {
    map(
        tuple((
            parse_ident,
            delimited(parse_ws, tag("="), parse_ws),
            parse_quoted,
        )),
        |(attr, _, pattern)| RawExpr::Term(TermSpec { attr, pattern }),
    )(inp)
}

fn parse_paren(inp: &S) -> IResult<&S, RawExpr, VErr<'_>> {
    delimited(tag("("), parse_or_expr, tag(")"))(inp)
}

fn parse_factor(inp: &S) -> IResult<&S, RawExpr, VErr<'_>> {
    delimited(parse_ws, alt((parse_paren, parse_term)), parse_ws)(inp)
}

fn parse_and_expr(inp: &S) -> IResult<&S, RawExpr, VErr<'_>> {
    let (mut inp, mut out) = parse_factor(inp)?;
    loop {
        match preceded(tuple((parse_ws, tag("&"), parse_ws)), parse_factor)(inp) {
            Ok((next, rhs)) => {
                out = RawExpr::And(Box::new(out), Box::new(rhs));
                inp = next;
            }
            Err(nom::Err::Error(_)) => break,
            Err(e) => return Err(e),
        }
    }
    Ok((inp, out))
}

fn parse_or_expr(inp: &S) -> IResult<&S, RawExpr, VErr<'_>> {
    let (mut inp, mut out) = parse_and_expr(inp)?;
    loop {
        match preceded(tuple((parse_ws, tag("|"), parse_ws)), parse_and_expr)(inp) {
            Ok((next, rhs)) => {
                out = RawExpr::Or(Box::new(out), Box::new(rhs));
                inp = next;
            }
            Err(nom::Err::Error(_)) => break,
            Err(e) => return Err(e),
        }
    }
    Ok((inp, out))
}

fn parse_root(inp: &S) -> IResult<&S, RawExpr, VErr<'_>> {
    all_consuming(delimited(parse_ws, parse_or_expr, parse_ws))(inp)
}

fn intern_expr(
    expr: RawExpr,
    terms: &mut Vec<TermSpec>,
    map: &mut HashMap<(String, String), usize>,
) -> Expr {
    match expr {
        RawExpr::Term(term) => {
            let key = (term.attr.clone(), term.pattern.clone());
            let idx = if let Some(idx) = map.get(&key).copied() {
                idx
            } else {
                let idx = terms.len();
                terms.push(term);
                map.insert(key, idx);
                idx
            };
            Expr::Term(idx)
        }
        RawExpr::And(a, b) => Expr::And(
            Box::new(intern_expr(*a, terms, map)),
            Box::new(intern_expr(*b, terms, map)),
        ),
        RawExpr::Or(a, b) => Expr::Or(
            Box::new(intern_expr(*a, terms, map)),
            Box::new(intern_expr(*b, terms, map)),
        ),
    }
}

pub fn parse_spec(inp: &S) -> Result<ParsedSpec, String> {
    match parse_root(inp).finish() {
        Ok((_rest, raw)) => {
            let mut terms = Vec::new();
            let mut map = HashMap::<(String, String), usize>::new();
            let expr = intern_expr(raw, &mut terms, &mut map);
            Ok(ParsedSpec { expr, terms })
        }
        Err(e) => Err(convert_error(inp, e)),
    }
}

pub fn eval_expr(expr: &Expr, term_truth: &[bool]) -> bool {
    match expr {
        Expr::Term(idx) => term_truth[*idx],
        Expr::And(a, b) => eval_expr(a, term_truth) && eval_expr(b, term_truth),
        Expr::Or(a, b) => eval_expr(a, term_truth) || eval_expr(b, term_truth),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_before_or() {
        let parsed = parse_spec("a=\"x\"&b=\"y\"|c=\"z\"").expect("must parse");
        assert_eq!(parsed.terms.len(), 3);
        assert!(matches!(parsed.expr, Expr::Or(_, _)));
    }

    #[test]
    fn parentheses_override_precedence() {
        let parsed = parse_spec("(a=\"x\"|b=\"y\")&c=\"z\"").expect("must parse");
        assert_eq!(parsed.terms.len(), 3);
        assert!(matches!(parsed.expr, Expr::And(_, _)));
    }

    #[test]
    fn nested_parentheses_parse() {
        let parsed = parse_spec("((a=\"x\"))|b=\"y\"").expect("must parse");
        assert!(matches!(parsed.expr, Expr::Or(_, _)));
    }

    #[test]
    fn duplicate_term_is_interned() {
        let parsed = parse_spec("(a=\"x\")|a=\"x\"").expect("must parse");
        assert_eq!(parsed.terms.len(), 1);
    }

    #[test]
    fn escaped_quotes_parse() {
        let parsed = parse_spec(r#"a="x\"y" | b='u\'v'"#).expect("must parse");
        assert_eq!(parsed.terms.len(), 2);
        assert_eq!(parsed.terms[0].pattern, "x\"y");
        assert_eq!(parsed.terms[1].pattern, "u'v");
    }

    #[test]
    fn unmatched_parenthesis_fails() {
        assert!(parse_spec("a=\"x\")").is_err());
        assert!(parse_spec("(a=\"x\"").is_err());
    }

    #[test]
    fn missing_value_fails() {
        assert!(parse_spec("a=").is_err());
    }
}
