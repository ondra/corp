mod lex;
mod corp;
mod text;
mod bits;
mod rev;
mod structure;


fn main() {
    let corpname = std::env::args().nth(1).expect("specify corpname");
    eprintln!("{}", corpname);

    let mut corp = corp::Corpus::open(&corpname).expect("corpus open failed");
    let mut attr = corp.open_attribute("word").expect("attribute open failed");

    //println!("{}", &corp.conf);
    println!("{}", corp.conf.value("PATH").unwrap_or("PATH not found"));
    println!("{:?}", attr);


    let mut it = attr.text.at(100);

    for i in 100..200 {
        let id = it.next().unwrap();
        //println!("{:?}: {} {}",i, id, attr.lex.id2str(id as u32) )
    }

    println!("{:?}", &attr.rev);

    let id = attr.lex.str2id("county");
    println!("{}", id);
    println!("");

    let mut revit = attr.rev.id2poss(id as u32);
    for p in revit {
        println!("{}", p);
    }


    //println!("{:#?}", parse_blk(&buf));
    // println!("{}", parse_blk(&buf));
    //let (_rest, elems) = parse_blk(&buf)?;
    //println!("{}", elems);
}
