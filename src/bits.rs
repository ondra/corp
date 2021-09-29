#[derive(Debug)]
pub struct Reader<'a> {
    bitpos: usize,
    mem: &'a[u64]
}

impl <'a> Reader<'_> {
    pub fn open(mem: &'a[u64], offset_bits: usize) -> Reader<'a> {
        Reader { mem, bitpos: offset_bits }
    }

    pub fn skip_bits(&mut self, n: usize) { self.bitpos += n; }
    pub fn tell(&self) -> i64 { self.bitpos as i64 }
    fn atom(&self) -> u64 {
        let p1 = self.mem[self.bitpos/64];
        let p2 = if self.bitpos/64+1 < self.mem.len() {
            self.mem[self.bitpos/64+1] 
        } else {
            0
        };
        (p1 >> (self.bitpos%64))
            | if self.bitpos%64 == 0 { 0 } else { p2 << (64 - self.bitpos%64) }
    }

    pub fn delta(&mut self) -> u64 {
        let mut atom = self.atom();
        let unarylen = atom.trailing_zeros() as u64;
        atom >>= unarylen + 1;
        let exponent = (atom & ((1 << unarylen) - 1)) | (1 << unarylen);
        atom >>= unarylen;
        self.skip_bits(((unarylen + 1) + unarylen + (exponent - 1)) as usize);
        (atom & ((1 << (exponent - 1)) - 1)) | (1 << (exponent - 1))
    }

    pub fn gamma(&mut self) -> u64 {
        let mut atom = self.atom();
        let unarylen = atom.trailing_zeros() as u64;
        atom >>= unarylen + 1;
        self.skip_bits(((unarylen + 1) + unarylen) as usize);
        (atom & ((1 << unarylen) - 1)) | (1 << unarylen)
    }

    pub fn bit(&mut self) -> bool {
        let v = (self.atom() & 1) != 0;
        self.skip_bits(1);
        v
    }
}
