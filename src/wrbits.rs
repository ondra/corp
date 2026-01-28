use std::cmp::min;
use std::fs::File;
use std::io::{BufWriter, Write};

type Atom = usize;

pub struct BitsWriter {
    part: Atom,
    freebits: usize,
    target: BufWriter<File>,
    total_bits: u64,
}

impl BitsWriter {
    pub fn new(target: BufWriter<File>) -> BitsWriter {
        BitsWriter {
            part: 0,
            freebits: Atom::BITS as usize,
            target,
            total_bits: 0,
        }
    }

    pub fn freebits(&self) -> usize {
        self.freebits
    }

    pub fn usedbits(&self) -> usize {
        Atom::BITS as usize - self.freebits
    }

    pub fn bits_written(&self) -> u64 {
        self.total_bits + self.usedbits() as u64
    }

    pub fn byte_align(&mut self) {
        let rem = self.bits_written() % 8;
        if rem == 0 {
            return;
        }
        let pad = 8 - rem;
        for _ in 0..pad {
            self.bit(false);
        }
    }

    pub fn delta(&mut self, val: u64) {
        assert!(val > 0);
        let mut len = u64::BITS as usize - val.leading_zeros() as usize;
        self.gamma(len as u64);
        let mut rest = (val & !(1 << (len - 1))) as Atom;
        len -= 1;
        while len > 0 {
            self.reserve();
            let curatom_len = min(self.freebits(), len);
            self.part |= rest << self.usedbits();
            self.freebits -= curatom_len;
            rest >>= curatom_len;
            len -= curatom_len;
        }
    }

    pub fn gamma(&mut self, val: u64) {
        assert!(val > 0);
        let mut len = u64::BITS as usize - val.leading_zeros() as usize;
        self.unary(len as u64);
        let mut rest = (val & !(1 << (len - 1))) as Atom;
        len -= 1;
        while len > 0 {
            self.reserve();
            let curatom_len = min(self.freebits(), len);
            self.part |= rest << self.usedbits();
            self.freebits -= curatom_len;
            rest >>= curatom_len;
            len -= curatom_len;
        }
    }

    pub fn unary(&mut self, val: u64) {
        assert!(val > 0);
        let mut len = (val - 1) as usize;
        while len > 0 {
            self.reserve();
            let curatom_len = min(self.freebits(), len);
            self.freebits -= curatom_len;
            len -= curatom_len;
        }
        self.bit(true);
    }

    pub fn bit(&mut self, val: bool) {
        self.reserve();
        if val {
            self.part |= 1 << self.usedbits();
        }
        self.freebits -= 1;
    }

    fn reserve(&mut self) {
        if self.freebits == 0 {
            self.emit(self.part);
            self.part = 0;
            self.freebits = Atom::BITS as usize;
        }
    }

    fn emit(&mut self, part: Atom) {
        self.target.write_all(&part.to_le_bytes()).unwrap();
        self.total_bits += Atom::BITS as u64;
    }

    pub fn finish(mut self) -> Result<BufWriter<File>, Box<dyn std::error::Error>> {
        if self.usedbits() > 0 {
            let num_bytes = (self.usedbits() + 7) / 8;
            self.target.write_all(&self.part.to_le_bytes()[0..num_bytes])?;
            self.total_bits += (num_bytes * 8) as u64;
        }
        self.target.flush()?;
        Ok(self.target)
    }
}
