use corp::corp::Corpus;
use corp::wsketch::WMap;
use corp::wsketch::WSLex;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let corpname = std::env::args().nth(1).unwrap();
    let corp = Corpus::open(&corpname)?;

    let wsattrname = corp.get_conf("WSATTR").unwrap();
    let wsattr = corp.open_attribute(&wsattrname)?;
    let wsbase = corp.get_conf("WSBASE").unwrap();
    let ws = WMap::new(&wsbase)?;
    let wslex = WSLex::open(&wsbase, wsattr.as_ref())?;
    // let r = ws.find_id(1); println!("a {:?}", r);

    if let Some(head_str) = std::env::args().nth(2) {
        let head_id = wsattr.str2id(&head_str).ok_or("head not found in lexicon")?;
        let head = ws.find_id(head_id).ok_or("head not found in word sketch")?;
        for rel in head.iter() {
            for coll in rel.iter() {
                println!("{}\t{}\t{}\t{}\t{}",
                         wslex.id2head(head.id), wslex.id2rel(rel.id), wslex.id2coll(coll.id),
                         coll.cnt, coll.rnk);
                // for (pos, collrelpos) in c {
                //     println!("-- # {} {}", pos, collrelpos.unwrap_or(9999));
                // }
            }
        }
    } else {
        for head in ws.iter_ids() {
            for rel in head.iter() {
                for coll in rel.iter() {
                    println!("{}\t{}\t{}\t{}\t{}",
                             wslex.id2head(head.id), wslex.id2rel(rel.id), wslex.id2coll(coll.id),
                             coll.cnt, coll.rnk);
                // for (pos, collrelpos) in c {
                //     println!("-- # {} {}", pos, collrelpos.unwrap_or(9999));
                // }
                }
            }
        };
    }

    Ok(())
}
