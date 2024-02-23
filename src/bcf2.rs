use byteorder::{LittleEndian, ReadBytesExt};
use splitty;
use std::ops::Range;
use std::{collections::HashMap, io::Seek};

#[derive(Debug)]
pub struct Header {
    dict_strings: Vec<HashMap<String, String>>,
    dict_contigs: Vec<HashMap<String, String>>,
    samples: Vec<String>,
    fmt_gt_idx: usize,
}
impl Header {
    pub fn from_string(text: &str) -> Self {
        let mut dict_strings = Vec::<HashMap<String, String>>::new();
        let mut dict_contigs = Vec::<HashMap<String, String>>::new();
        let mut samples = Vec::<String>::new();

        // implicit FILTER/PASS header lines
        let mut m = HashMap::<String, String>::new();
        m.insert("Dictionary".into(), "FILTER".into());
        m.insert("ID".into(), "PASS".into());
        m.insert("Description".into(), r#""All filters passed""#.into());
        dict_strings.push(m);
        //
        for line in text.trim_end_matches('\0').trim().split("\n") {
            if line.starts_with("#CHROM") {
                line.split("\t")
                    .skip(8)
                    .for_each(|s| samples.push(s.into()));
                continue;
            } else if line.trim().len() == 0 {
                continue;
            }
            let mut it = line.strip_prefix("##").unwrap().split("=");
            let dict_name = it.next().unwrap();
            let valid_dict = match it.next() {
                Some(x) if x.starts_with("<") => true,
                _ => false,
            };
            if !valid_dict {
                continue;
            }
            let l = line.find('<').unwrap();
            let s = line.split_at(l + 1).1;
            let r = s.rfind('>').unwrap();
            let s = s.split_at(r).0;
            let mut m = HashMap::<String, String>::new();
            for kv_str in s.split(",") {
                let kv_str = kv_str.trim();
                let mut it = splitty::split_unquoted_char(kv_str, '=').unwrap_quotes(true);
                let k = it.next().unwrap();
                let v = it.next().unwrap();
                m.insert(k.into(), v.into());
            }
            match dict_name {
                "contig" => dict_contigs.push(m),
                _ => {
                    if (dict_name == "FILTER") && (&m["ID"] == "PASS") {
                        // skip FILTER/PASS already added
                    } else {
                        m.insert("Dictionary".into(), dict_name.into());
                        dict_strings.push(m)
                    }
                }
            };
        }

        // reorder items if the header line has IDX key
        let mut fmt_gt_idx = 0;
        for (idx, m) in dict_strings.iter().enumerate() {
            if (&m["Dictionary"] == "FORMAT") && (&m["ID"] == "GT") {
                fmt_gt_idx = idx;
            }
        }

        Self {
            dict_strings,
            dict_contigs,
            samples,
            fmt_gt_idx,
        }
    }

    pub fn get_chrname(&self, idx: usize) -> &str {
        &self.dict_contigs[idx]["ID"]
    }
    pub fn get_fmt_gt_id(&self) -> usize {
        self.fmt_gt_idx
    }
    pub fn get_contigs(&self) -> &Vec<HashMap<String, String>> {
        &self.dict_contigs
    }
    pub fn get_dict_strings(&self) -> &Vec<HashMap<String, String>> {
        &self.dict_strings
    }
    pub fn get_samples(&self) -> &Vec<String> {
        &self.samples
    }
}

pub trait Bcf2Number {
    fn is_missing(&self) -> bool;
    fn is_end_of_vector(&self) -> bool;
    fn is_reserved_value(&self) -> bool;
}

impl Bcf2Number for u8 {
    fn is_missing(&self) -> bool {
        *self == 0x80
    }
    fn is_end_of_vector(&self) -> bool {
        *self == 0x81
    }
    fn is_reserved_value(&self) -> bool {
        (*self >= 0x80) && (*self <= 0x87)
    }
}

impl Bcf2Number for u16 {
    fn is_missing(&self) -> bool {
        *self == 0x8000
    }
    fn is_end_of_vector(&self) -> bool {
        *self == 0x8001
    }
    fn is_reserved_value(&self) -> bool {
        (*self >= 0x8000) && (*self <= 0x8007)
    }
}

impl Bcf2Number for u32 {
    fn is_missing(&self) -> bool {
        *self == 0x80000000
    }
    fn is_end_of_vector(&self) -> bool {
        *self == 0x80000001
    }
    fn is_reserved_value(&self) -> bool {
        (*self >= 0x80000000) && (*self <= 0x80000007)
    }
}
impl Bcf2Number for f32 {
    fn is_missing(&self) -> bool {
        (*self) as u32 == 0x7FC00000
    }
    fn is_end_of_vector(&self) -> bool {
        (*self) as u32 == 0x7FC00001
    }
    fn is_reserved_value(&self) -> bool {
        ((*self) as u32 >= 0x7FC00001) && ((*self) as u32 <= 0x7FC00007)
    }
}

pub fn bcf2_typ_width(typ: u8) -> usize {
    match typ {
        0x0 => 0,
        0x1 => 1,
        0x2 => 2,
        0x3 => 3,
        0x5 => 3,
        0x7 => 1,
        _ => panic!(),
    }
}

pub enum NumbericValue {
    U8(u8),
    U16(u16),
    U32(u32),
    F32(f32),
}

impl From<u8> for NumbericValue {
    fn from(value: u8) -> Self {
        Self::U8(value)
    }
}
impl From<u16> for NumbericValue {
    fn from(value: u16) -> Self {
        Self::U16(value)
    }
}
impl From<u32> for NumbericValue {
    fn from(value: u32) -> Self {
        Self::U32(value)
    }
}
impl From<f32> for NumbericValue {
    fn from(value: f32) -> Self {
        Self::F32(value)
    }
}

impl NumbericValue {
    pub fn int_val(&self) -> Option<u32> {
        match *self {
            Self::U8(x) if !x.is_missing() => Some(x as u32),
            Self::U16(x) if !x.is_missing() => Some(x as u32),
            Self::U32(x) if !x.is_missing() => Some(x as u32),
            _ => None,
        }
    }
    pub fn float_val(&self) -> Option<f32> {
        match *self {
            Self::F32(x) if !x.is_missing() => Some(x),
            _ => None,
        }
    }

    pub fn gt_val(&self) -> (bool, bool, bool, u32) {
        let mut noploidy = false;
        let mut dot = false;
        let mut phased = false;
        let mut allele = u32::MAX;

        match self.int_val() {
            None => {
                noploidy = true;
            }
            Some(int_val) if int_val + 1 == 0 => {
                dot = true;
            }
            Some(int_val) => {
                phased = (int_val & 0x1) != 0;
                allele = (int_val >> 1) - 1;
            }
        };

        (noploidy, dot, phased, allele)
    }
}

pub fn read_typed_descriptor_bytes<R>(reader: &mut R) -> (u8, usize)
where
    R: std::io::Read + ReadBytesExt,
{
    let tdb = reader.read_u8().unwrap();
    let typ = tdb & 0xf;
    let mut n = (tdb >> 4) as usize;
    if n == 15 {
        n = read_single_typed_integer(reader) as usize;
    }
    (typ, n)
}

pub fn read_single_typed_integer<R>(reader: &mut R) -> u32
where
    R: std::io::Read + ReadBytesExt,
{
    let (typ, n) = read_typed_descriptor_bytes(reader);
    assert_eq!(n, 1);
    match typ {
        1 => reader.read_u8().unwrap() as u32,
        2 => reader.read_u16::<LittleEndian>().unwrap() as u32,
        3 => reader.read_u32::<LittleEndian>().unwrap(),
        _ => panic!(),
    }
}

#[derive(Default, Debug)]
pub struct NumberIter<'r> {
    reader: std::io::Cursor<&'r [u8]>,
    typ: u8,
    len: usize,
    cur: usize,
}

impl<'r> Iterator for NumberIter<'r> {
    type Item = NumbericValue;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur >= self.len {
            None
        } else {
            match self.typ {
                0 => None,
                1 => {
                    self.cur += 1;
                    Some(self.reader.read_u8().unwrap().into())
                }
                2 => {
                    self.cur += 1;
                    Some(self.reader.read_u16::<LittleEndian>().unwrap().into())
                }
                3 => {
                    self.cur += 1;
                    Some(self.reader.read_u32::<LittleEndian>().unwrap().into())
                }
                5 => {
                    self.cur += 1;
                    Some(self.reader.read_f32::<LittleEndian>().unwrap().into())
                }
                _ => panic!(),
            }
        }
    }
}

pub fn iter_typed_integers(typ: u8, n: usize, buffer: &[u8]) -> NumberIter {
    NumberIter {
        reader: std::io::Cursor::new(buffer),
        typ,
        len: n,
        cur: 0,
    }
}

/// if 0 is return, it means the string is missing
pub fn read_typed_string<R>(reader: &mut R, buffer: &mut Vec<u8>) -> usize
where
    R: std::io::Read + ReadBytesExt,
{
    let (typ, n) = read_typed_descriptor_bytes(reader);
    assert_eq!(typ, 0x7);
    let s = buffer.len();
    buffer.resize(s + n, b'\0');
    reader.read(&mut buffer.as_mut_slice()[s..s + n]).unwrap();
    n
}

pub fn read_header<R>(reader: &mut R) -> String
where
    R: std::io::Read + ReadBytesExt,
{
    // read magic
    let mut magic = [0u8; 3];
    reader.read(&mut magic).unwrap();
    assert_eq!(&magic, b"BCF");

    // read major verion and minor version
    let major = reader.read_u8().unwrap();
    let minor = reader.read_u8().unwrap();
    assert_eq!(major, 2);
    assert_eq!(minor, 2);

    // read text length
    let l_length = reader.read_u32::<LittleEndian>().unwrap();
    let mut text = vec![0u8; l_length as usize];
    reader.read(&mut text).unwrap();

    String::from_utf8(text).unwrap()
}

#[derive(Default, Debug)]
pub struct Record {
    buf_site: Vec<u8>,
    buf_gt: Vec<u8>,
    chrom: i32,
    pos: i32,
    rlen: i32,
    qual: f32,
    n_info: u16,
    n_allele: u16,
    n_sample: u32,
    n_fmt: u8,
    id: Range<usize>,
    alleles: Vec<Range<usize>>,
    /// (typ, n, byte_range)
    filters: (u8, usize, Range<usize>),
    /// (info_key, typ, n, byte_range)
    info: Vec<(usize, u8, usize, Range<usize>)>,
    /// (fmt_key, typ, n, byte_range)
    gt: Vec<(usize, u8, usize, Range<usize>)>,
}
impl Record {
    /// read a record, copy bytes and separate fields
    pub fn read<R>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>>
    where
        R: std::io::Read + ReadBytesExt,
    {
        let l_shared;
        let l_indv;
        l_shared = match reader.read_u32::<LittleEndian>() {
            Ok(x) => x,
            Err(_x) => Err(_x)?,
        };
        l_indv = reader.read_u32::<LittleEndian>()?;
        self.buf_site.resize(l_shared as usize, 0u8);
        self.buf_gt.resize(l_indv as usize, 0u8);
        reader.read_exact(self.buf_site.as_mut_slice()).unwrap();
        reader.read_exact(self.buf_gt.as_mut_slice()).unwrap();
        self.parse_site_fields();
        self.parse_gt_fields();
        Ok(())
    }
    /// parse shared fields, complicated field will need further processing
    fn parse_site_fields(&mut self) {
        let mut reader = std::io::Cursor::new(self.buf_site.as_slice());
        self.chrom = reader.read_i32::<LittleEndian>().unwrap();
        self.pos = reader.read_i32::<LittleEndian>().unwrap();
        self.rlen = reader.read_i32::<LittleEndian>().unwrap();
        self.qual = reader.read_f32::<LittleEndian>().unwrap();
        self.n_info = reader.read_u16::<LittleEndian>().unwrap();
        self.n_allele = reader.read_u16::<LittleEndian>().unwrap();
        let combined = reader.read_u32::<LittleEndian>().unwrap();
        self.n_sample = combined & 0xffffff;
        self.n_fmt = (combined >> 24) as u8;
        // id
        let (typ, n) = read_typed_descriptor_bytes(&mut reader);
        assert_eq!(typ, 0x7);
        let cur = reader.position() as usize;
        self.id = cur..cur + n as usize;
        reader.seek(std::io::SeekFrom::Current(n as i64)).unwrap();
        // alleles
        self.alleles.clear();
        for _ in 0..self.n_allele {
            let (typ, n) = read_typed_descriptor_bytes(&mut reader);
            assert_eq!(typ, 0x7);
            let cur = reader.position() as usize;
            self.alleles.push(cur..cur + n as usize);
            reader.seek(std::io::SeekFrom::Current(n as i64)).unwrap();
        }
        //filters
        let (typ, n) = read_typed_descriptor_bytes(&mut reader);
        let width: usize = bcf2_typ_width(typ);
        let s = reader.position() as usize;
        let e = s + width * n as usize;
        reader
            .seek(std::io::SeekFrom::Current((e - s) as i64))
            .unwrap();
        self.filters = (typ, n as usize, s..e);
        // infos
        self.info.clear();
        for _idx in 0..(self.n_info as usize) {
            let info_key = read_single_typed_integer(&mut reader);
            let (typ, n) = read_typed_descriptor_bytes(&mut reader);
            let width = bcf2_typ_width(typ);
            let s = reader.position() as usize;
            let e = width * n as usize;
            reader
                .seek(std::io::SeekFrom::Current((e - s) as i64))
                .unwrap();
            self.info.push((info_key as usize, typ, n as usize, s..e));
        }
    }
    /// parse shared fields, complicated field will need further processing
    fn parse_gt_fields(&mut self) {
        let mut reader = std::io::Cursor::new(self.buf_gt.as_slice());
        self.gt.clear();
        for _idx in 0..(self.n_fmt as usize) {
            let fmt_key = read_single_typed_integer(&mut reader);
            let (typ, n) = read_typed_descriptor_bytes(&mut reader);
            let width = bcf2_typ_width(typ);
            let s = reader.position() as usize;
            let e = s + width * self.n_sample as usize * n as usize;
            reader
                .seek(std::io::SeekFrom::Current((e - s) as i64))
                .unwrap();
            self.gt.push((fmt_key as usize, typ, n as usize, s..e));
        }
    }

    /// get chromosome offset
    pub fn chrom(&self) -> i32 {
        self.chrom
    }
    pub fn rlen(&self) -> i32 {
        self.rlen
    }
    pub fn qual(&self) -> Option<f32> {
        match self.qual.is_missing() {
            true => None,
            false => Some(self.qual),
        }
    }
    pub fn gt(&self, header: &Header) -> NumberIter<'_> {
        let fmt_gt_id = header.get_fmt_gt_id();
        // default iterator
        let mut it = NumberIter::default();

        // find the right field for gt
        self.gt.iter().for_each(|e| {
            if e.0 == fmt_gt_id {
                it = iter_typed_integers(
                    e.1,
                    e.2 as usize * self.n_sample as usize,
                    &self.buf_gt[e.3.start..e.3.end],
                );
            }
        });
        it
    }
}

#[test]
fn test_read_gt() {
    let mut f = std::fs::File::open("test_flat.bcf").unwrap();
    let s = read_header(&mut f);
    let header = Header::from_string(&s);
    let mut record = Record::default();

    // let mut buf = vec![0u8; 0];

    let mut cnt0 = 0;
    let mut cnt1 = 0;
    let mut cnt2 = 0;
    while let Ok(_) = record.read(&mut f) {
        eprintln!("{cnt2}");
        cnt2+=1;
        // use std::io::Write;
        for bn in record.gt(&header) {
            // write!(buf, "{}",bn.gt_val().3 ).unwrap();
            // let allele = bn.gt_val().3;
            let allele = 0;
            if allele == 0 {
                cnt0 += 1;
            } else {
                cnt1 += 1;
            }
        }
        // write!(buf, "\n").unwrap();
    }
    // let buf = String::from_utf8(buf).unwrap();
    // let buf2 = std::fs::read_to_string("test_gt.txt").unwrap();
    // assert_eq!(buf, buf2);
    eprintln!("cnt0= {cnt0}, cnt1={cnt1}");
}
