//use crate::{lex,corp,text,bits,rev,structure};

use corp::corp;
//use corp;
/*
mod lex;
mod corp;
mod text;
mod bits;
mod rev;
mod structure;
*/

fn main() {
    let corpname = std::env::args().nth(1).expect("specify corpname");
    eprintln!("{}", corpname);

    let corp = corp::Corpus::open(&corpname).expect("corpus open failed");
    let attr = corp.open_attribute("word").expect("attribute open failed");

    //println!("{}", &corp.conf);
    println!("{}", corp.conf.value("PATH").unwrap_or("PATH not found"));
    println!("{:?}", attr);


    let mut it = attr.iter_ids(100);

    for i in 100..200 {
        let id = it.next().unwrap();
        println!("{:?}: {} {}",i, id, attr.id2str(id as u32) )
    }

    //println!("{:?}", &attr.rev);

    let id = attr.str2id("test").unwrap_or(u32::MAX);
    println!("{}", id);
    println!("");

    /*
    let revit = attr.rev.id2poss(id as u32);
    for p in revit {
        let itt = attr.iter_ids(p).next().unwrap();
        println!("{} {}", p, itt);
    }*/


    //println!("{:#?}", parse_blk(&buf));
    // println!("{}", parse_blk(&buf));
    //let (_rest, elems) = parse_blk(&buf)?;
    //println!("{}", elems);
}
