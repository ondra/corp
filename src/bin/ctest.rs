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

    let s = corp.open_struct("doc").expect("structure open failed");
    println!("{:?}", s);
    let st = corp.open_structtext("doc", "month").expect("text open failed");
    println!("{:?}", st);

    let mut it = attr.iter_ids(100);

    /*
    for i in 100..200 {
        let id = it.next().unwrap();
        println!("{:?}: {} {} {:?}",i, id, attr.id2str(id as u32), s.find_beg(i, 0) )
    }
    */

    println!();
    let mut laststructno: Option<u64> = None;
    let mut tot = 0;
    for dn in (0..attr.text().size() as u64).step_by(2) {
        let i = dn;
        // let mut it = attr.iter_ids(i);
        // let id = it.next().unwrap();
        let beg = s.find_beg(i,
                             // laststructno.unwrap_or(0)
                             0
                             );
        // println!("{:?}: {} {} {:?}", i, id, attr.id2str(id as u32), beg )
        tot += beg.unwrap_or(0);
    }

    println!("{}", tot);
    //println!("{:?}", &attr.rev);

    let id = attr.str2id("test").unwrap_or(u32::MAX);
    println!("{}", id);
    println!();

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
