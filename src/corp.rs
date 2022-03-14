use fs_err::File;
use std::io::Read;
use std::fmt;

use crate::lex;
use crate::text;
use crate::rev;

#[derive(Debug)]
pub struct Attribute {
    path: String,
    name: String,
    conf: corpconf::Block,
    pub lex: lex::MapLex,
    pub text: Box<dyn text::Text>,
    pub rev: Box<dyn rev::Rev>,
}

impl Attribute {}

#[derive(Debug)]
pub struct Corpus {
    path: String,
    name: String,
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
        Ok(Corpus{
            path: rebase_path(conf_filename, conf.value("PATH").ok_or(AttrNotFound{})?)?.to_string(),
            name: conf_filename.to_string(),
            conf
        })
    }

    pub fn open_attribute(&self, name: &str) -> Result<Attribute, Box<dyn std::error::Error>> {
        let attr = self.conf.attribute(name).ok_or(AttrNotFound{})?;
        Ok(Attribute{
            path: self.path.clone() + "/" + name,
            name: name.to_string(),
            conf: attr.clone(),
            lex: lex::MapLex::open(&(self.path.clone() + "/" + name))?,
            text: self.open_text(
                &(self.path.clone() + "/" + name),
                attr.value("TYPE").unwrap_or("MD_MD"))?,
            rev: rev::open(&(self.path.clone() + "/" + name))?,
        })
    }

    fn open_text(&self, path: &str, typecode: &str)
        -> Result<Box<dyn text::Text>, Box<dyn std::error::Error>>
    {
        match typecode {
            "MD_MD" | "FD_FD" | "FD_MD"
                => Ok(Box::new(text::Delta::open(path)?)),
            "MD_MGD" | "FD_FGD" | "FD_MGD"
                => Ok(Box::new(text::GigaDelta::open(path)?)),
            _ => Err(Box::new(AttrNotFound{}))
        }
    }
}

