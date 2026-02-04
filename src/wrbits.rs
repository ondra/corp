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

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::BitsWriter;
    use crate::bits::Reader;
    use std::fs::File;
    use std::io::BufWriter;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("corp_wrbits_test_{}_{}_{}", std::process::id(), n, name))
    }

    fn read_file_words(path: &std::path::Path) -> Vec<u64> {
        let mut bytes = std::fs::read(path).unwrap();
        if bytes.is_empty() {
            bytes.resize(8, 0);
        } else if bytes.len() % 8 != 0 {
            let pad = 8 - (bytes.len() % 8);
            bytes.resize(bytes.len() + pad, 0);
        }
        bytes
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn write_with<F>(name: &str, f: F) -> (Vec<u64>, u64)
    where
        F: FnOnce(&mut BitsWriter),
    {
        let path = tmp_path(name);
        let file = File::create(&path).unwrap();
        let bw = BufWriter::new(file);
        let mut w = BitsWriter::new(bw);
        f(&mut w);
        let bits = w.bits_written();
        let _ = w.finish().unwrap();
        let words = read_file_words(&path);
        let _ = std::fs::remove_file(&path);
        (words, bits)
    }

    fn read_unary(r: &mut Reader<'_>) -> u64 {
        let mut zeros = 0u64;
        while !r.bit() {
            zeros += 1;
        }
        zeros + 1
    }

    #[test]
    fn bit_roundtrip_pattern_crosses_word_boundary() {
        let mut expected = Vec::new();
        let (mem, _bits) = write_with("bit_pattern.bin", |w| {
            for i in 0..257usize {
                let b = i % 3 == 0 || i % 17 == 0;
                expected.push(b);
                w.bit(b);
            }
        });

        let mut r = Reader::open(&mem, 0);
        for (i, &b) in expected.iter().enumerate() {
            assert_eq!(r.bit(), b, "bit mismatch at index {}", i);
        }
    }

    #[test]
    fn unary_roundtrip_various_lengths() {
        let values: Vec<u64> = (1..=150).collect();
        let (mem, _bits) = write_with("unary.bin", |w| {
            for &v in &values {
                w.unary(v);
            }
        });

        let mut r = Reader::open(&mem, 0);
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(read_unary(&mut r), v, "unary mismatch at index {}", i);
        }
    }

    #[test]
    fn gamma_roundtrip_small_and_large() {
        let mut values = vec![1u64, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 63, 64, 65, 127, 128, 129];
        values.extend([1u64 << 10, (1u64 << 10) + 1, 1u64 << 20, (1u64 << 20) + 123]);

        let (mem, _bits) = write_with("gamma.bin", |w| {
            for &v in &values {
                w.gamma(v);
            }
        });

        let mut r = Reader::open(&mem, 0);
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(r.gamma(), v, "gamma mismatch at index {}", i);
        }
    }

    #[test]
    fn delta_roundtrip_small_and_large() {
        let mut values = vec![1u64, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 63, 64, 65, 127, 128, 129];
        values.extend([1u64 << 10, (1u64 << 10) + 1, 1u64 << 20, (1u64 << 20) + 123]);

        let (mem, _bits) = write_with("delta.bin", |w| {
            for &v in &values {
                w.delta(v);
            }
        });

        let mut r = Reader::open(&mem, 0);
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(r.delta(), v, "delta mismatch at index {}", i);
        }
    }

    #[test]
    fn byte_align_pads_with_zero_bits() {
        let (mem, _bits) = write_with("align.bin", |w| {
            // Write a non-byte-aligned prefix.
            for i in 0..13usize {
                w.bit(i % 2 == 0);
            }
            let before = w.bits_written();
            assert_ne!(before % 8, 0);
            w.byte_align();
            let after = w.bits_written();
            assert_eq!(after % 8, 0);
            assert!(after >= before);
        });

        let mut r = Reader::open(&mem, 0);
        // First 13 bits are the pattern.
        for i in 0..13usize {
            assert_eq!(r.bit(), i % 2 == 0);
        }
        // Remaining bits up to the next byte boundary are zeros.
        let pad = (8 - (13 % 8)) % 8;
        for _ in 0..pad {
            assert!(!r.bit());
        }
    }
}
