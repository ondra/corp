use fs_err::File;
use std::io::Read;
use std::fmt;

use crate::lex;
use crate::text;
use crate::rev;
use crate::structure;

use crate::text::Text;

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

fn open_freq(base: &str, kind: &str) -> Result<Box<dyn Frequency>, Box<dyn std::error::Error>> {
    if kind.contains(":") {
        let mut parts = kind.split(":");
        let ext = parts.next().ok_or(format!("bad frequency kind: {}", kind))?;
        let datatype = parts.next().ok_or(format!("bad frequency kind: {}", kind))?;
        match datatype {
            "l" => Ok(Box::new(FromFile::<u64>::open(&(base.to_string() + "." + ext))?)),
            _ => Err(format!("bad frequency type: {}", kind).into()),
        }
    } else { match kind {
        "frq" => {
            if std::path::Path::new(&(base.to_string() + ".frq64")).exists() {
                Ok(Box::new(FromFile::<u64>::open(&(base.to_string() + ".frq64"))?))
            } else {
                Ok(Box::new(FromFile::<u32>::open(&(base.to_string() + ".frq"))?))
            }
        },
        _ => Err(format!("bad frequency type: {}", kind).into()),
    }}
}

struct FromFile<T> {
    map: memmap::Mmap,
    _marker: std::marker::PhantomData<T>,
}

impl <T> FromFile<T> where T: Copy {
    fn open(path: &str) -> Result<FromFile<T>, Box<dyn std::error::Error>> {
        let f = File::open(path)?;
        Ok(FromFile::<T>{
            map: unsafe { memmap::MmapOptions::new().map(f.file())? },
            //_marker: Default::default(),
            _marker: std::marker::PhantomData,
        })
    }
    fn at(&self, id: u32) -> T { as_slice_ref(&self.map)[id as usize] }
}

impl <T> Frequency for FromFile<T> where T: Copy, u64: From<T> {
    fn frq(&self, id: u32) -> u64 { self.at(id).into() }
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
    fn get_freq(&self, t: &str) -> Result<Box<dyn Frequency + '_>, Box<dyn std::error::Error>>;
}

impl Attr for StdAttr {
    fn iter_ids(&self, frompos: u64) -> Box<dyn Iterator<Item=u32> + '_> {
        let pa = self.text.posat(frompos);
        if pa.is_some() { Box::new(pa.unwrap()) }
        else { Box::new(self.text.structat(frompos).unwrap()) }
    }
    fn id2str(&self, id: u32) -> &str { self.lex.id2str(id) }
    fn str2id(&self, s: &str) -> Option<u32> { self.lex.str2id(s) }
    fn revidx(&self) -> &dyn rev::Rev { self.rev.as_ref() }
    fn text(&self) -> &dyn text::Text { self.text.as_ref() }
    fn id_range(&self) -> u32 { self.lex.id_range() }
    fn get_freq(&self, t: &str) -> Result<Box<dyn Frequency + '_>, Box<dyn std::error::Error>> {
        match t {
            "frq" => Ok(Box::new(RevFrequency { a: &self })),
            _ => open_freq(&self.path, t),
        }
    }
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
        Box::new(DynIter {di: it, da: self})
        //Box::new(vec![1u32, 2, 3].into_iter())
    }
    fn id2str(&self, id: u32) -> &str { self.lex.id2str(id) }
    fn str2id(&self, s: &str) -> Option<u32> { self.lex.str2id(s) }
    fn revidx(&self) -> &dyn rev::Rev { self.fromattr.revidx() }
    fn text(&self) -> &dyn text::Text { return self }
    fn id_range(&self) -> u32 { self.lex.id_range() }
    fn get_freq(&self, t: &str) -> Result<Box<dyn Frequency + '_>, Box<dyn std::error::Error>> {
        match t {
            "frq" => Ok(Box::new(DynFrequency{ da: &self })),
            _ => open_freq(&self.path, t),
        }
    }
}

impl Text for DynAttr {
    fn posat(&self, _pos: u64) -> Option<text::DeltaIter<'_>> { panic!() }
    fn structat(&self, _pos: u64) -> Option<text::IntIter<'_>> { panic!() }
    fn size(&self) -> usize { self.fromattr.text().size() }
    fn get(&self, pos: u64) -> u32 { as_slice_ref(&self.ridx)[self.fromattr.text().get(pos) as usize] }
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

struct DynFrequency<'a> { pub da: &'a DynAttr, }
impl Frequency for DynFrequency<'_> {
    fn frq(&self, id: u32) -> u64 {
        let mut tot = 0u64;
        for oid in self.da.lrev.id2poss(id) {
            tot += self.da.fromattr.frq(oid as u32)
        }
        tot
    }
}

struct RevFrequency<'a> { pub a: &'a StdAttr, }
impl Frequency for RevFrequency<'_> {
    fn frq(&self, id: u32) -> u64 { self.a.rev.count(id) }
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
    Ok(if path.starts_with('.') {
        let canonical_conf_filename = std::fs::canonicalize(conf_filename)?;
        let mut dirname = canonical_conf_filename.parent().unwrap().to_path_buf();
        dirname.push(path);
        dirname.to_string_lossy().to_string()
    } else {
        path.to_string()
    })
}

const FALLBACK_MANATEE_REGISTRY: [&str; 1] = ["/corpora/registry/"];
fn get_registry_paths() -> Vec<String> {
    std::env::var("MANATEE_REGISTRY")
        .unwrap_or("".to_string())
        .split(":")
        .filter(|s| !s.is_empty())
        .chain(FALLBACK_MANATEE_REGISTRY)
        .map(|s| {s.to_string()})
        .collect::<Vec<String>>()
}

fn find_config(corpname: &str) -> Result<String, Box<dyn std::error::Error>> {
    if corpname.starts_with(".") { // cwd-relative path
        Ok(corpname.to_string())
    } else if corpname.starts_with("/") { // absolute path, do nothing
        Ok(corpname.to_string())
    } else { // name relative to MANATEE_REGISTRY
        for path in get_registry_paths() {
            let fullpath = std::path::Path::new(&path).join(corpname);
            if fullpath.is_file() {
                return Ok(fullpath.to_string_lossy().into_owned());
            }
        }
        Err("could not find the corpus configuration file in MANATEE_REGISTRY".into())
    }
}

impl Corpus {
    pub fn open(corpname: &str) -> Result<Corpus, Box<dyn std::error::Error>> {
        let conf_filename = find_config(&corpname)?;
        let mut file = File::open(&conf_filename)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        let conf = corpconf::parse_conf_opt(&buf)?;
        let path = rebase_path(&conf_filename, conf.value("PATH").ok_or(AttrNotFound{})?)?;
        let path = path.trim_end_matches('/').to_string() + "/";
        Ok(Corpus{ path, name: conf_filename, conf })
    }

    pub fn rebase_path(&self, path: &str) -> Result<String, Box<dyn std::error::Error>> {
        rebase_path(&self.name, path)
    }

    pub fn open_attribute<'a, 'b>(&'a self, name: &str) -> Result<Box<dyn Attr + Sync + Send + 'b>, Box<dyn std::error::Error>> 
    {
        let path = self.path.clone() + "/" + name;

        let attrconf = if name.contains('.') {
            let mut parts = name.split('.');
            let structconf = self.conf.structure(parts.next().unwrap())
                .ok_or(AttrNotFound{})?;
            structconf.attribute(parts.next().unwrap())
        } else {
            self.conf.attribute(name)
        }.ok_or(AttrNotFound{})?;

        if let Some(_dynamic) = attrconf.value("DYNAMIC") {
            let fromattrname = attrconf.value("FROMATTR").ok_or(AttrNotFound{})?;
            let fromattr = if name.contains(".") {
                let fa = name.split(".").next().unwrap().to_string()
                    + "." + fromattrname;
                self.open_attribute(&fa)
            } else {
                self.open_attribute(fromattrname)
            }?;

            let ridxf = File::open(path.clone() + ".lex.ridx")?;
            // let frqf = File::open(path.clone() + ".frq")?;

            Ok(Box::new(DynAttr{
                path,
                name: name.to_string(),
                conf: attrconf.clone(),
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
                conf: attrconf.clone(),
                lex: lex::MapLex::open(&(self.path.clone() + "/" + name))?,
                text: self.open_text(
                    &(self.path.clone() + "/" + name),
                    if name.contains('.') {
                        attrconf.value("TYPE").unwrap_or("Int")
                    } else {
                        attrconf.value("TYPE").unwrap_or("MD_MD")
                    })?,
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
            "Int"
                => Ok(Box::new(text::Int::open(path)?)),
            _ => Err(Box::new(AttrNotFound{}))
        }
    }

    pub fn open_struct<'a>(&self, name: &str)
        -> Result<Box<dyn structure::Struct + Sync + Send + 'a>, Box<dyn std::error::Error>>
    {
        let s = self.conf.structure(name).ok_or(AttrNotFound{})?;
        let type64 = matches!(s.value("TYPE"), Some("file64") | Some("map64"));
        structure::open(
            &(self.path.clone() + "/" + name),
            type64
        )
    }

    pub fn open_structtext<'a>(&self, structname: &str, attrname: &str)
        -> Result<text::Int, Box<dyn std::error::Error>>
    {
        Ok(text::Int::open(&(self.path.clone() + "/" + structname + "." + attrname))?)
    }

    pub fn get_conf(&self, name: &str) -> Option<String> {
        if let Some(val) = self.conf.value(name) {
            if val != "" {
                return Some(val.to_string());
            }
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

