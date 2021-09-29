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
    pub text: text::Delta,
    pub rev: rev::Delta,
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

impl Corpus {
    pub fn open(conf_filename: &str) -> Result<Corpus, Box<dyn std::error::Error>> {
        let mut file = File::open(&conf_filename)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        let conf = corpconf::parse_conf_opt(&buf)?;
        Ok(Corpus{
            path: conf.value("PATH").ok_or(AttrNotFound{})?.to_string(),
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
            text: text::Delta::open(&(self.path.clone() + "/" + name))?,
            rev: rev::Delta::open(&(self.path.clone() + "/" + name))?,
        })
    }
}

