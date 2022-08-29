use std::fs::File;
use std::io::BufRead;

use std::hash::Hash;
use std::hash::Hasher;

use std::collections::hash_map::DefaultHasher;

use std::collections::HashMap;
use std::mem::size_of;

use std::cmp::{min};
use std::io::BufWriter;
use std::io::Write;
use std::io::Seek;
use std::io::SeekFrom;

type Atom = usize;

pub struct BinFile {
    f: BufWriter<File>,
}

impl BinFile {
    pub fn new(path: &str) -> Result<BinFile, Box<dyn std::error::Error>> {
        let f = File::create(path)?;
        let bw = BufWriter::new(f);

        Ok(BinFile{f: bw})
    }

    pub fn put(&mut self, val: u64) -> Result<(), Box<dyn std::error::Error>> {
        self.f.write(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn finalize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
       Ok(self.f.flush()?)
    }

}

pub struct WriteBits {
    part: Atom,
    _freebits: usize,
    target: BufWriter<File>,
}

impl WriteBits {
    fn new(target: BufWriter<File>) -> WriteBits {
        WriteBits {
            part: 0,
            _freebits: size_of::<Atom>()*8,
            target,
        }
    }

    fn freebits(&self) -> usize {
        self._freebits
    }

    fn usedbits(&self) -> usize {
        size_of::<Atom>()*8 - self._freebits
    }

    // write a single Elias delta-coded value consisting of an
    // gamma-encoded length followed by the binary value
    fn delta(&mut self, val: u64) {
        assert!(val > 0);
        let mut len = size_of::<u64>()*8 - val.leading_zeros() as usize; 
        self.gamma(len as u64);
        let mut rest = ( val & !(1<<(len-1)) ) as Atom;
        len -= 1;
        while len > 0 {
            self.reserve();
            let curatom_len = min(self.freebits(), len);
            self.part |= rest << self.usedbits();
            self._freebits -= curatom_len;
            rest = rest >> curatom_len;
            len -= curatom_len;
        }
    }

    // write a single Elias gamma-coded value consisting of an
    // unary-encoded length followed by the binary value
    fn gamma(&mut self, val: u64) {
        assert!(val > 0);
        let mut len = size_of::<u64>()*8 - val.leading_zeros() as usize; 
        self.unary(len as u64);
        let mut rest = ( val & !(1<<(len-1)) ) as Atom;
        len -= 1;
        while len > 0 {
            self.reserve();
            let curatom_len = min(self.freebits(), len);
            self.part |= rest << self.usedbits();
            self._freebits -= curatom_len;
            rest = rest >> curatom_len;
            len -= curatom_len;
        }
    }

    // write a single bit
    fn bit(&mut self, val: bool) {
        self.reserve();
        if val {
            self.part |= 1 << self.usedbits();
        }
        self._freebits -= 1;
    }

    // ensure that there is some free space in the buffer
    fn reserve(&mut self) {
        if self.freebits() == 0 {
            self.emit(self.part);
            self.part = 0;
            self._freebits = size_of::<Atom>()*8;
        }
    }

    // write val 0-bits followed by one 1-bit
    fn unary(&mut self, val: u64) {
        assert!(val<=63);
        assert!(val > 0);

        let mut len = (val - 1) as usize;

        while len > 0 {
            self.reserve();
            let curatom_len = min(self.freebits(), len);
            self._freebits -= curatom_len;
            len -= curatom_len;
        }

        self.bit(true);
    }

    // write the remaining contents of the buffer
    fn finish(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.usedbits() > 0 {
            let num_bytes = (self.usedbits()+7) % 8;
            self.target.write(&self.part.to_le_bytes()[0..num_bytes])?;
        }
        self.target.flush()?;
        Ok(())
    }

    // output a single atom
    fn emit(&mut self, part: Atom) {
        self.target.write(&part.to_le_bytes()).unwrap(); // FIXME unwrap
        // eprintln!("atom {}", part);
    }

    fn into(self) -> BufWriter<File> {
        self.target
    }
}

fn main() {
    let arg1 = std::env::args().nth(1).expect("arg1");

    let stdin = std::io::stdin();
    let mut r : Box<dyn BufRead> = if arg1 == "-" {
        Box::new(stdin.lock())
    } else {
        let f = File::open(arg1).expect("x");
        Box::new(std::io::BufReader::new(f))
    };

    let mut h = DefaultHasher::new();

    let mut lex = HashMap::<String, u32>::new();

    let mut nextid = 0u32;

    let mut wb = BinFile::new("/tmp/xyz.text").unwrap();
    wb.f.seek(SeekFrom::Start(0)).unwrap();
    wb.f.write(&[0xa3u8, 'f' as u8, 'i' as u8, 'n' as u8, 'D' as u8, 'T' as u8]).unwrap();

    wb.f.seek(SeekFrom::Start(32)).unwrap();

    let mut wl = WriteBits::new(wb.f);
    let mut position = 0u64;

    let mut buf = String::new();

    for _lineno in 1.. {
        buf.clear();

        match r.read_line(&mut buf) {
            Ok(0) => { break },
            Ok(_n) => {},
            Err(e) => { eprintln!("error: {}", e); break },
        };

        // strip end of line
        if buf.ends_with('\n') {
            buf.pop();
            if buf.ends_with('\r') {
                buf.pop();
            }
        }
        
        let id = match lex.get(&buf) {
            Some(id) => { *id },
            None => {
                lex.insert(buf.clone(), nextid);
                let curid = nextid;
                nextid = nextid + 1;
                curid
            }
        };

        wl.delta(id as u64 + 1);
        position += 1;

        Hash::hash(&id, &mut h);
        //eprintln!("{}", id);

    }
    wl.finish().unwrap();
    let mut wb = wl.into();
    wb.seek(SeekFrom::Start(16)).unwrap();
    let mut wl = WriteBits::new(wb);

    let segment_size = 128;
    let text_size = position;

    wl.delta(segment_size+1);
    wl.delta(text_size+1);

    wl.finish().unwrap();


    eprintln!("hash {}", h.finish());
}
