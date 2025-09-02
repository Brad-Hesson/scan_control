use std::{
    fs::{File, read},
    io::{BufRead, BufReader, Cursor, Read},
    path::{self, Path},
    process::id,
};

use itertools::Itertools;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[derive(Debug)]
pub struct SXM {
    metadata: Vec<(Box<str>, Box<str>)>,
    data: Vec<Box<[f32]>>,
}

impl SXM {
    #[inline]
    pub fn parse_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::parse(BufReader::new(std::fs::File::open(path)?))
    }
    pub fn parse(mut reader: impl BufRead) -> std::io::Result<Self> {
        let mut out = Self {
            metadata: vec![],
            data: vec![],
        };
        let mut meta = Vec::new();
        reader.read_until(0x04, &mut meta)?;
        let mut input = meta.as_slice();
        let mut key = chop_line(&mut input).unwrap().trim_matches(':');
        let mut value = String::new();
        while let Some(line) = chop_line(&mut input) {
            if line.starts_with(':') && line.ends_with(':') {
                out.metadata.push((
                    key.to_string().into_boxed_str(),
                    value[..value.len() - 1]
                        .to_string()
                        .clone()
                        .into_boxed_str(),
                ));
                key = line.trim_matches(':');
                value.clear();
            } else {
                value.push_str(line);
                value.push('\n');
            }
        }
        assert_eq!(
            out.get_metadata("SCANIT_TYPE").map(|v| v.replace(" ", "")),
            Some("FLOATMSBFIRST".into())
        );
        let (pix_x, pix_y) = out.get_image_size();
        let mut read_buf = [0u8; 4];
        loop {
            let mut data_buf = Vec::with_capacity(pix_x * pix_y);
            if let Err(_) = reader.read_exact(&mut read_buf) {
                break;
            }
            data_buf.push(f32::from_be_bytes(read_buf));
            for _ in 1..pix_x * pix_y {
                reader.read_exact(&mut read_buf)?;
                data_buf.push(f32::from_be_bytes(read_buf));
            }
            out.data.push(data_buf.into_boxed_slice());
        }
        Ok(out)
    }
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata
            .iter()
            .find_map(|(k, v)| (&**k == key).then_some(&**v))
    }
    pub fn get_image_size(&self) -> (usize, usize) {
        self.get_metadata("SCAN_PIXELS")
            .unwrap()
            .split_ascii_whitespace()
            .map(str::parse::<usize>)
            .map(Result::unwrap)
            .collect_tuple()
            .unwrap()
    }
}

fn chop_line<'a>(input: &mut &'a [u8]) -> Option<&'a str> {
    if input.is_empty() {
        return None;
    }
    let end = input
        .iter()
        .position(|c| *c == b'\n')
        .unwrap_or(input.len());
    let out = std::str::from_utf8(&input[..end]).unwrap();
    if end + 1 < input.len() {
        *input = &input[end + 1..];
    } else {
        *input = &[];
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn it_works() -> Result<(), Box<dyn Error>> {
        let sxm = SXM::parse_file("20240229_066.sxm")?;
        dbg!(sxm.data.len());
        for d in sxm.data{
            println!("{} ... {}", d.first().unwrap(), d.last().unwrap());
        }
        Ok(())
    }

    #[test]
    fn test_chop_line() {
        let mut s = r#"h
        "#
        .as_bytes();
        while let Some(line) = chop_line(&mut s) {
            println!("line: {line}");
        }
        println!("rest: {s:?}")
    }
}
