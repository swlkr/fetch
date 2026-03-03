use crate::{Result, error};
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;

pub struct ChunkIter<'a, R: Read> {
    pub reader: &'a mut BufReader<R>,
    pub done: bool,
}

impl<'a, R: Read> Iterator for ChunkIter<'a, R> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut size_line = String::new();
        if let Err(e) = self.reader.read_line(&mut size_line) {
            return Some(Err(error(e)));
        }
        let size_str = size_line.trim();
        let size_str = size_str.split(';').next().unwrap_or("0");
        let size = match usize::from_str_radix(size_str, 16) {
            Ok(s) => s,
            Err(e) => return Some(Err(error(e))),
        };
        if size == 0 {
            self.done = true;
            let mut trailing = String::new();
            let _ = self.reader.read_line(&mut trailing);
            return None;
        }
        let mut buf = vec![0u8; size];
        if let Err(e) = self.reader.read_exact(&mut buf) {
            return Some(Err(error(e)));
        }
        let mut crlf = [0u8; 2];
        if let Err(e) = self.reader.read_exact(&mut crlf) {
            return Some(Err(error(e)));
        }
        Some(Ok(buf))
    }
}
