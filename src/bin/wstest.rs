//use crate::{lex,corp,text,bits,rev,structure};

use corp::corp::Corpus;
use corp::wsketch::WMap;
use corp::lex::MapLex;

fn main() {
    let corpname = std::env::args().nth(1).expect("specify corpname");
    let corp = Corpus::open(&corpname).expect("corpus open failed");

    let wsattrname = "lemma".to_string();
    let wsattr = corp.open_attribute(&wsattrname).expect("attribute open failed");

    let mut wsbase = corp.path.to_string();
    wsbase += "/";
    wsbase += &wsattrname;
    wsbase += "-ws";
    // println!("wsbase {}", &wsbase);
    let ws = WMap::new(&wsbase).expect("failed to open WMap");
    
    let grlex = MapLex::open(&wsbase).expect("failed to open gramrel lexicon");

    // let r = ws.find_id(1);
    // println!("a {:?}", r);

    for head in ws.iter_ids() {
        for rel in head.iter() {
            for coll in rel.iter() {
                println!("{}\t{}\t{}\t{}\t{}",
                         wsattr.id2str(head.id), grlex.id2str(rel.id), wsattr.id2str(coll.id),
                         coll.cnt, coll.rnk);
                // for (pos, collrelpos) in c {
                //     println!("-- # {} {}", pos, collrelpos.unwrap_or(9999));
                // }
            }
        }
    };

}
