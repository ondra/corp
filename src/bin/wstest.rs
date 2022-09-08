//use crate::{lex,corp,text,bits,rev,structure};

use corp::corp::Corpus;
use corp::wsketch::WMap;
use corp::lex::MapLex;

fn main() {
    let corpname = std::env::args().nth(1).expect("specify corpname");
    let corp = Corpus::open(&corpname).expect("corpus open failed");

    //let wsattrname = "lemma".to_string();
    let wsattrname = corp.get_conf("WSATTR").unwrap();
    let wsattr = corp.open_attribute(&wsattrname).expect("attribute open failed");
    /*
    let mut wsbase = corp.path.to_string();
    wsbase += "/";
    wsbase += &wsattrname;
    wsbase += "-ws";
    */
    let wsbase = corp.get_conf("WSBASE").unwrap();
    println!("wsbase {}", &wsbase);
    let ws = WMap::new(&wsbase).expect("failed to open WMap");
    
    let grlex = MapLex::open(&wsbase).expect("failed to open gramrel lexicon");

    let ml = MapLex::open(&(wsbase.to_string() + ".coll"));
    let colllex = match ml {
        Ok(a) => Some(a),
        Err(e@std::io::Error {..}) => {
            if e.kind() == std::io::ErrorKind::InvalidInput { None }
            else { std::panic::panic_any(e) }
        },
    };

    let id2coll = |id| {
        if id > wsattr.id_range() {
            let cl = if let Some(ref cl) = colllex { cl } else { panic!() };
            cl.id2str(id - wsattr.id_range()).to_string()
        } else {
            wsattr.id2str(id).to_string()
        }
    };

    // let r = ws.find_id(1);
    // println!("a {:?}", r);

    for head in ws.iter_ids() {
        for rel in head.iter() {
            for coll in rel.iter() {
                println!("{}\t{}\t{}\t{}\t{}",
                         wsattr.id2str(head.id), grlex.id2str(rel.id), id2coll(coll.id),
                         coll.cnt, coll.rnk);
                // for (pos, collrelpos) in c {
                //     println!("-- # {} {}", pos, collrelpos.unwrap_or(9999));
                // }
            }
        }
    };

}
