//! Native JS5 disk cache reader. Input files are read-only; group CRCs are checked.
use crate::binary::Reader;
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, serde::Serialize)]
pub struct Group {
    pub name_hash: Option<u32>,
    pub crc: u32,
    pub version: u32,
    pub files: Vec<u32>,
}
pub struct Cache {
    root: PathBuf,
    data: File,
    indices: HashMap<u8, BTreeMap<u32, Group>>,
    pub verified: BTreeMap<String, u32>,
}
fn open(root: &Path, name: &str) -> Result<File> {
    File::open(root.join(name))
        .or_else(|_| File::open(root.join(format!("{name}.dat"))))
        .with_context(|| format!("Opening {}", root.join(name).display()))
}
impl Cache {
    pub fn new(root: &Path) -> Result<Self> {
        let root = if root.join("main_file_cache.dat2").is_file()
            || root.join("main_file_cache.dat2.dat").is_file()
        {
            root.to_owned()
        } else {
            root.join("LIVE")
        };
        Ok(Self {
            data: open(&root, "main_file_cache.dat2")?,
            root,
            indices: HashMap::new(),
            verified: BTreeMap::new(),
        })
    }
    pub fn raw(&mut self, archive: u8, group: u32) -> Result<Vec<u8>> {
        let mut idx = open(&self.root, &format!("main_file_cache.idx{archive}"))?;
        idx.seek(SeekFrom::Start(group as u64 * 6))?;
        let mut entry = [0; 6];
        idx.read_exact(&mut entry)?;
        let mut r = Reader::new(&entry);
        let size = r.u24()? as usize;
        let mut sector = r.u24()?;
        ensure!(
            size > 0 && size <= 32 * 1024 * 1024,
            "Invalid group size {size}"
        );
        let mut result = Vec::with_capacity(size);
        let mut seen = HashSet::new();
        let mut chunk = 0u16;
        let large = group > 65535;
        let header = if large { 10 } else { 8 };
        while result.len() < size {
            ensure!(
                sector > 0 && seen.insert(sector),
                "Invalid/cyclic sector chain"
            );
            self.data.seek(SeekFrom::Start(sector as u64 * 520))?;
            let mut bytes = vec![0; header];
            self.data.read_exact(&mut bytes)?;
            let mut r = Reader::new(&bytes);
            let identity = if large { r.u32()? } else { r.u16()? as u32 };
            ensure!(
                identity == group && r.u16()? == chunk,
                "Group/chunk identity mismatch for {archive}/{group}"
            );
            sector = r.u24()?;
            ensure!(r.u8()? == archive, "Archive identity mismatch");
            let count = (520 - header).min(size - result.len());
            let start = result.len();
            result.resize(start + count, 0);
            self.data.read_exact(&mut result[start..])?;
            chunk = chunk.checked_add(1).context("Too many sectors")?;
        }
        Ok(result)
    }
    pub fn index(&mut self, archive: u8) -> Result<BTreeMap<u32, Group>> {
        if let Some(index) = self.indices.get(&archive) {
            return Ok(index.clone());
        }
        let data = decode(&self.raw(255, archive as u32)?)?.0;
        let index = reference(&data)?;
        self.indices.insert(archive, index.clone());
        Ok(index)
    }
    pub fn file(&mut self, archive: u8, group: u32, file: u32) -> Result<Vec<u8>> {
        self.group(archive, group)?
            .remove(&file)
            .with_context(|| format!("Missing file {archive}/{group}/{file}"))
    }
    pub fn group(&mut self, archive: u8, group: u32) -> Result<BTreeMap<u32, Vec<u8>>> {
        let definition = self
            .index(archive)?
            .get(&group)
            .cloned()
            .with_context(|| format!("Missing group {archive}/{group}"))?;
        let raw = self.raw(archive, group)?;
        let (data, end) = decode(&raw)?;
        let crc = crc32fast::hash(&raw[..end]);
        ensure!(crc == definition.crc, "CRC mismatch in {archive}/{group}");
        self.verified.insert(format!("{archive}/{group}"), crc);
        split(&data, &definition.files)
    }
}
pub fn decode(raw: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut r = Reader::new(raw);
    let compression = r.u8()?;
    let length = r.u32()? as usize;
    ensure!(
        length <= 32 * 1024 * 1024,
        "Compressed container exceeds limit"
    );
    if compression == 0 {
        return Ok((r.take(length)?.to_vec(), 5 + length));
    }
    let unpacked = r.u32()? as usize;
    ensure!(
        unpacked <= 128 * 1024 * 1024,
        "Uncompressed container exceeds limit"
    );
    let payload = r.take(length)?;
    let mut data = vec![];
    match compression {
        1 => {
            let mut body = vec![];
            if !payload.starts_with(b"BZh") {
                body.extend_from_slice(b"BZh1")
            }
            body.extend_from_slice(payload);
            bzip2::read::BzDecoder::new(&body[..])
                .take(unpacked as u64 + 1)
                .read_to_end(&mut data)?;
        }
        2 => {
            flate2::read::GzDecoder::new(payload)
                .take(unpacked as u64 + 1)
                .read_to_end(&mut data)?;
        }
        n => bail!("Unsupported cache compression {n}"),
    }
    ensure!(data.len() == unpacked, "Unpacked cache length mismatch");
    Ok((data, 9 + length))
}
pub fn reference(bytes: &[u8]) -> Result<BTreeMap<u32, Group>> {
    let mut r = Reader::new(bytes);
    let protocol = r.u8()?;
    ensure!(
        (5..=7).contains(&protocol),
        "Unsupported reference protocol {protocol}"
    );
    if protocol >= 6 {
        r.u32()?;
    }
    let flags = r.u8()?;
    ensure!(flags & !1 == 0, "Unsupported reference flags {flags}");
    let count = r.id(protocol)? as usize;
    ensure!(count <= 100000, "Reference table too large");
    let mut ids = vec![];
    let mut id = 0u32;
    for _ in 0..count {
        id = id
            .checked_add(r.id(protocol)?)
            .context("Group ID overflow")?;
        ids.push(id);
    }
    let names = if flags & 1 != 0 {
        (0..count)
            .map(|_| r.u32().map(Some))
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![None; count]
    };
    let mut index = BTreeMap::new();
    for (&id, name_hash) in ids.iter().zip(names) {
        index.insert(
            id,
            Group {
                name_hash,
                crc: r.u32()?,
                version: 0,
                files: vec![],
            },
        );
    }
    for id in &ids {
        index.get_mut(id).unwrap().version = r.u32()?;
    }
    let counts = (0..count).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
    for (&id, &count) in ids.iter().zip(&counts) {
        let mut file = 0u32;
        for _ in 0..count {
            file = file
                .checked_add(r.id(protocol)?)
                .context("File ID overflow")?;
            index.get_mut(&id).unwrap().files.push(file);
        }
    }
    if flags & 1 != 0 {
        for count in counts {
            r.take(count as usize * 4)?;
        }
    }
    r.finish()?;
    Ok(index)
}
pub fn split(data: &[u8], ids: &[u32]) -> Result<BTreeMap<u32, Vec<u8>>> {
    ensure!(!ids.is_empty(), "Empty group");
    if ids.len() == 1 {
        return Ok(BTreeMap::from([(ids[0], data.to_vec())]));
    }
    let chunks = *data.last().context("Empty multi-file group")? as usize;
    let table = chunks
        .checked_mul(ids.len())
        .and_then(|n| n.checked_mul(4))
        .context("Chunk table overflow")?;
    ensure!(data.len() > table, "Truncated chunk table");
    let start = data.len() - table - 1;
    let mut r = Reader::new(&data[start..data.len() - 1]);
    let mut cursor = 0;
    let mut result: BTreeMap<_, _> = ids.iter().map(|&id| (id, vec![])).collect();
    for _ in 0..chunks {
        let mut length = 0i64;
        for id in ids {
            length += r.i32()? as i64;
            ensure!(
                length >= 0 && length as usize <= start - cursor,
                "Invalid group chunk delta"
            );
            result
                .get_mut(id)
                .unwrap()
                .extend_from_slice(&data[cursor..cursor + length as usize]);
            cursor += length as usize;
        }
    }
    ensure!(cursor == start, "Unconsumed group payload");
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_signed_deltas() {
        let mut data = b"abcdef".to_vec();
        data.extend_from_slice(&4i32.to_be_bytes());
        data.extend_from_slice(&(-2i32).to_be_bytes());
        data.push(1);
        let parts = split(&data, &[0, 9]).unwrap();
        assert_eq!(parts[&0], b"abcd");
        assert_eq!(parts[&9], b"ef");
    }
    #[test]
    fn compression_roundtrip() {
        use std::io::Write;
        let bytes = b"real native bzip2 validation";
        let mut encoder = bzip2::write::BzEncoder::new(vec![], bzip2::Compression::best());
        encoder.write_all(bytes).unwrap();
        let packed = encoder.finish().unwrap();
        let mut container = vec![1];
        container.extend_from_slice(&((packed.len() - 4) as u32).to_be_bytes());
        container.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        container.extend_from_slice(&packed[4..]);
        assert_eq!(decode(&container).unwrap().0, bytes);
        assert!(decode(&container[..container.len() - 1]).is_err());
    }
}
