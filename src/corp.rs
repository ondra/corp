use fs_err::File;
use std::io::Read;
use std::fmt;

use crate::lex;
use crate::text;
use crate::rev;
use crate::structure;

use crate::util::as_slice_ref;

#[derive(Debug)]
pub struct StdAttr {
    pub path: String,
    pub name: String,
    pub conf: corpconf::Block,
    pub lex: lex::MapLex,
    pub text: Box<dyn text::Text + Sync + Send>,
    pub rev: Box<dyn rev::Rev + Sync + Send>,
}

#[derive(Debug)]
pub struct DynAttr {
//pub struct DynAttr<'a, 'b> {
    pub path: String,
    pub name: String,
    pub conf: corpconf::Block,
    pub lex: lex::MapLex,
    pub fromattr: Box<dyn Attr + Sync + Send>,
    ridx: memmap::Mmap,

    // frqm: memmap::Mmap,
    lrev: Box<dyn rev::Rev + Sync + Send>,
}

// pub trait Attr<'a> : std::fmt::Debug + Frequency {
pub trait Attr: std::fmt::Debug + Frequency + Sync + Send {
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item=u32> + '_>;
    fn id2str(&self, id: u32) -> &str;
    fn str2id(&self, s: &str) -> Option<u32>;
    fn revidx(&self) -> &dyn rev::Rev;
    fn text(&self) -> &dyn text::Text;
    fn id_range(&self) -> u32;
}

impl Attr for StdAttr {
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item=u32> + '_> {
        Box::new(self.text.at(frompos))
    }
    fn id2str(&self, id: u32) -> &str { self.lex.id2str(id) }
    fn str2id(&self, s: &str) -> Option<u32> { self.lex.str2id(s) }
    fn revidx(&self) -> &dyn rev::Rev { self.rev.as_ref() }
    fn text(&self) -> &dyn text::Text { self.text.as_ref() }
    fn id_range(&self) -> u32 { self.lex.id_range() }
}

struct DynIter<'a> {
    di: Box<dyn Iterator<Item=u32> + 'a>,
    da: &'a DynAttr,
}

impl Iterator for DynIter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if let Some(orgid) = self.di.next() {
            Some(as_slice_ref(&self.da.ridx)[orgid as usize])
        } else { None }
    }
}

impl Attr for DynAttr {
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item=u32> + '_> {
        let it = self.fromattr.iter_ids(frompos);
        Box::new(DynIter {di: it, da: &self})
        //Box::new(vec![1u32, 2, 3].into_iter())
    }
    fn id2str(&self, id: u32) -> &str { self.lex.id2str(id) }
    fn str2id(&self, s: &str) -> Option<u32> { self.lex.str2id(s) }
    fn revidx(&self) -> &dyn rev::Rev { self.fromattr.revidx() }
    fn text(&self) -> &dyn text::Text { self.fromattr.text() }
    fn id_range(&self) -> u32 { self.lex.id_range() }
}

pub trait Frequency {
    fn frq(&self, id: u32) -> u64;
}

impl Frequency for DynAttr {
    // fn frq(&self, id: u32) -> u64 { as_slice_ref(&self.frqm)[id as usize] }
    fn frq(&self, id: u32) -> u64 {
        let mut tot = 0u64;
        for oid in self.lrev.id2poss(id) {
            tot += self.fromattr.frq(oid as u32)
        }
        tot
    }
}

impl Frequency for StdAttr {
    fn frq(&self, id: u32) -> u64 { self.rev.count(id) }
}

#[derive(Debug)]
pub struct Corpus {
    pub path: String,
    pub name: String,
    pub conf: corpconf::Block,
}

#[derive(Debug)]
struct AttrNotFound {}

impl fmt::Display for AttrNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AttrNotFound")
    }
}

impl std::error::Error for AttrNotFound {}

fn rebase_path(conf_filename: &str, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(if path.starts_with(".") {
        let canonical_conf_filename = std::fs::canonicalize(conf_filename)?;
        let mut dirname = canonical_conf_filename.parent().unwrap().to_path_buf();
        dirname.push(path);
        dirname.to_string_lossy().to_string()
    } else {
        path.to_string()
    })
}

impl Corpus {
    pub fn open(conf_filename: &str) -> Result<Corpus, Box<dyn std::error::Error>> {
        let mut file = File::open(&conf_filename)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        let conf = corpconf::parse_conf_opt(&buf)?;
        let path = rebase_path(conf_filename, conf.value("PATH").ok_or(AttrNotFound{})?)?;
        let path = path.trim_end_matches('/').to_string() + "/";
        Ok(Corpus{ path, name: conf_filename.to_string(), conf })
    }

    pub fn rebase_path(&self, path: &str) -> Result<String, Box<dyn std::error::Error>> {
        Ok(rebase_path(&self.name, path)?.to_string())
    }

    pub fn open_attribute<'a, 'b>(&'a self, name: &str) -> Result<Box<dyn Attr + Sync + Send + 'b>, Box<dyn std::error::Error>> 
    {
        let attr = self.conf.attribute(name).ok_or(AttrNotFound{})?;
        let path = self.path.clone() + "/" + name;
        if let Some(_dynamic) = attr.value("DYNAMIC") {
            let fromattrname = attr.value("FROMATTR").ok_or(AttrNotFound{})?;
            let fromattr = self.open_attribute(fromattrname.clone())?;

            let ridxf = File::open(path.clone() + ".lex.ridx")?;
            // let frqf = File::open(path.clone() + ".frq")?;

            Ok(Box::new(DynAttr{
                path,
                name: name.to_string(),
                conf: attr.clone(),
                lex: lex::MapLex::open(&(self.path.clone() + "/" + name))?,
                fromattr,
                //fromattr: self.open_attribute(fromattrname.clone())?,
                ridx: unsafe { memmap::MmapOptions::new().map(ridxf.file())? },
                // frqm: unsafe { memmap::MmapOptions::new().map(frqf.file())? },
                lrev: rev::open(&(self.path.clone() + "/" + name))?,
            }))
        } else {
            Ok(Box::new(StdAttr{
                path,
                name: name.to_string(),
                conf: attr.clone(),
                lex: lex::MapLex::open(&(self.path.clone() + "/" + name))?,
                text: self.open_text(
                    &(self.path.clone() + "/" + name),
                    attr.value("TYPE").unwrap_or("MD_MD"))?,
                rev: rev::open(&(self.path.clone() + "/" + name))?,
            }))
        }
    }

    fn open_text<'a>(&self, path: &str, typecode: &str)
        -> Result<Box<dyn text::Text + Sync + Send + 'a>, Box<dyn std::error::Error>>
    {
        match typecode {
            "MD_MD" | "FD_FD" | "FD_MD"
                => Ok(Box::new(text::Delta::open(path)?)),
            "MD_MGD" | "FD_FGD" | "FD_MGD"
                => Ok(Box::new(text::GigaDelta::open(path)?)),
            _ => Err(Box::new(AttrNotFound{}))
        }
    }

    pub fn open_struct<'a>(&self, name: &str)
        -> Result<Box<dyn structure::Struct + Sync + Send + 'a>, Box<dyn std::error::Error>>
    {
        let s = self.conf.structure(&name).ok_or(AttrNotFound{})?;
        let type64 = match s.value("TYPE") {
            Some("file64") => true,
            Some("map64") => true,
            _ => false,
        };

        structure::open(
            &(self.path.clone() + "/" + name),
            type64
        )
    }

    pub fn get_conf(&self, name: &str) -> Option<String> {
        if let Some(val) = self.conf.value(name) {
            return Some(val.to_string());
        }
        match name {
            "WSATTR" => {
                ["lempos_lc","lempos","lemma_lc","lemma"]
                .iter().find(
                    |a| self.conf.attribute(a).is_some()
                ).map(|x|x.to_string())
                .or_else(|| self.get_conf("DEFAULTATTR"))
            },
            "DEFAULTATTR" => Some("word".to_string()),
            "WSBASE" => Some(self.path.to_string()
                             + &self.get_conf("WSATTR").unwrap() + "-ws"),
            _ => None,
        }.map(|val| {
            match name {
                "WSBASE" => self.rebase_path(&val).unwrap(),
                _ => val,
            }
        })
    }
}

