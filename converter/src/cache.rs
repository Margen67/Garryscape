// SPDX-License-Identifier: AGPL-3.0-only
//! Native JS5 disk cache reader. Input files are read-only; group CRCs are checked.
use crate::binary::Reader;
use anyhow::{bail, ensure, Context as _, Result};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
};
/// Verified JS5 group metadata from an archive reference table.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct Group {
    /// Optional Java string hash used to find named map groups.
    pub name_hash: Option<u32>,
    /// Expected CRC32 of the encoded group container.
    pub crc: u32,
    /// Cache group revision from the reference table.
    pub version: u32,
    /// File identifiers in reference-table order.
    pub files: Vec<u32>,
}
/// Read-only JS5 disk cache with memoized archive metadata.
#[allow(
    clippy::partial_pub_fields,
    reason = "expose render metadata while encapsulating internal caches"
)]
pub(crate) struct Cache {
    /// Directory holding the native cache files.
    root: PathBuf,
    /// Shared sector data file.
    data: File,
    /// Decoded archive reference tables, keyed by archive ID.
    indices: HashMap<u8, BTreeMap<u32, Group>>,
    /// CRCs of groups successfully read during this conversion.
    pub verified: BTreeMap<String, u32>,
}
/// Open a native cache file, accepting the optional `.dat` suffix.
fn open(root: &Path, name: &str) -> Result<File> {
    File::open(root.join(name))
        .or_else(|_| File::open(root.join(format!("{name}.dat"))))
        .with_context(|| format!("Opening {}", root.join(name).display()))
}
impl Cache {
    /// Open a cache directory or its LIVE subdirectory.
    #[allow(
        clippy::shadow_reuse,
        reason = "local rebinding records successive numeric representations"
    )]
    pub(crate) fn new(root: &Path) -> Result<Self> {
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
    /// Read and validate the sector chain for one encoded group.
    #[allow(
        clippy::shadow_unrelated,
        reason = "each loop or conversion stage scopes its own intermediate values"
    )]
    pub(crate) fn raw(&mut self, archive: u8, group: u32) -> Result<Vec<u8>> {
        let mut idx = open(&self.root, &format!("main_file_cache.idx{archive}"))?;
        let _consumed = idx.seek(SeekFrom::Start(u64::from(group) * 6))?;
        let mut entry = [0; 6];
        idx.read_exact(&mut entry)?;
        let mut reader = Reader::new(&entry);
        let size = usize::try_from(reader.u24()?)?;
        let mut sector = reader.u24()?;
        ensure!(
            size > 0 && size <= 32 * 1024 * 1024,
            "Invalid group size {size}"
        );
        let mut result = Vec::with_capacity(size);
        let mut seen = HashSet::new();
        let mut chunk = 0_u16;
        let large = group > 65535;
        let header = if large { 10 } else { 8 };
        while result.len() < size {
            ensure!(
                sector > 0 && seen.insert(sector),
                "Invalid/cyclic sector chain"
            );
            let _consumed = self.data.seek(SeekFrom::Start(u64::from(sector) * 520))?;
            let mut bytes = vec![0; header];
            self.data.read_exact(&mut bytes)?;
            let mut reader = Reader::new(&bytes);
            let identity = if large {
                reader.u32()?
            } else {
                u32::from(reader.u16()?)
            };
            ensure!(
                identity == group && reader.u16()? == chunk,
                "Group/chunk identity mismatch for {archive}/{group}"
            );
            sector = reader.u24()?;
            ensure!(reader.u8()? == archive, "Archive identity mismatch");
            let count = 520_usize
                .saturating_sub(header)
                .min(size.saturating_sub(result.len()));
            let start = result.len();
            result.resize(start.checked_add(count).context("Group size overflow")?, 0);
            self.data
                .read_exact(result.get_mut(start..).context("Invalid sector range")?)?;
            chunk = chunk.checked_add(1).context("Too many sectors")?;
        }
        Ok(result)
    }
    /// Load an archive reference table, reusing its decoded metadata.
    pub(crate) fn index(&mut self, archive: u8) -> Result<BTreeMap<u32, Group>> {
        if let Some(index) = self.indices.get(&archive) {
            return Ok(index.clone());
        }
        let data = decode(&self.raw(255, u32::from(archive))?)?.0;
        let index = reference(&data)?;
        let _previous = self.indices.insert(archive, index.clone());
        Ok(index)
    }
    /// Extract one file from a verified group.
    pub(crate) fn file(&mut self, archive: u8, group: u32, file: u32) -> Result<Vec<u8>> {
        self.group(archive, group)?
            .remove(&file)
            .with_context(|| format!("Missing file {archive}/{group}/{file}"))
    }
    /// Verify a group CRC and split its decoded payload into files.
    pub(crate) fn group(&mut self, archive: u8, group: u32) -> Result<BTreeMap<u32, Vec<u8>>> {
        let definition = self
            .index(archive)?
            .get(&group)
            .cloned()
            .with_context(|| format!("Missing group {archive}/{group}"))?;
        let raw = self.raw(archive, group)?;
        let (data, end) = decode(&raw)?;
        let crc = crc32fast::hash(raw.get(..end).context("Invalid container range")?);
        ensure!(crc == definition.crc, "CRC mismatch in {archive}/{group}");
        let _previous = self.verified.insert(format!("{archive}/{group}"), crc);
        split(&data, &definition.files)
    }
}
/// Decompress a bounded JS5 container and return its encoded end offset.
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
pub(crate) fn decode(raw: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut reader = Reader::new(raw);
    let compression = reader.u8()?;
    let length = usize::try_from(reader.u32()?)?;
    ensure!(
        length <= 32 * 1024 * 1024,
        "Compressed container exceeds limit"
    );
    if compression == 0 {
        return Ok((
            reader.take(length)?.to_vec(),
            5_usize.saturating_add(length),
        ));
    }
    let unpacked = reader.u32()?;
    ensure!(
        unpacked <= 128 * 1024 * 1024,
        "Uncompressed container exceeds limit"
    );
    let payload = reader.take(length)?;
    let mut data = vec![];
    match compression {
        1 => {
            let mut body = vec![];
            if !payload.starts_with(b"BZh") {
                body.extend_from_slice(b"BZh1");
            }
            body.extend_from_slice(payload);
            let _consumed = bzip2::read::BzDecoder::new(&*body)
                .take(u64::from(unpacked).saturating_add(1))
                .read_to_end(&mut data)?;
        }
        2 => {
            let _consumed = flate2::read::GzDecoder::new(payload)
                .take(u64::from(unpacked).saturating_add(1))
                .read_to_end(&mut data)?;
        }
        value => bail!("Unsupported cache compression {value}"),
    }
    ensure!(
        data.len() == usize::try_from(unpacked)?,
        "Unpacked cache length mismatch"
    );
    Ok((data, 9_usize.saturating_add(length)))
}
/// Parse a revision-530-compatible archive reference table.
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
#[allow(
    clippy::shadow_unrelated,
    reason = "each loop or conversion stage scopes its own intermediate values"
)]
pub(crate) fn reference(bytes: &[u8]) -> Result<BTreeMap<u32, Group>> {
    let mut reader = Reader::new(bytes);
    let protocol = reader.u8()?;
    ensure!(
        (5..=7).contains(&protocol),
        "Unsupported reference protocol {protocol}"
    );
    if protocol >= 6 {
        let _consumed = reader.u32()?;
    }
    let flags = reader.u8()?;
    ensure!(flags & !1 == 0, "Unsupported reference flags {flags}");
    let count = usize::try_from(reader.id(protocol)?)?;
    ensure!(count <= 100_000, "Reference table too large");
    let mut ids = vec![];
    let mut id = 0_u32;
    for _ in 0..count {
        id = id
            .checked_add(reader.id(protocol)?)
            .context("Group ID overflow")?;
        ids.push(id);
    }
    let names = if flags & 1 != 0 {
        core::iter::repeat_with(|| reader.u32().map(Some))
            .take(count)
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![None; count]
    };
    let mut index = BTreeMap::new();
    for (&id, name_hash) in ids.iter().zip(names) {
        let _previous = index.insert(
            id,
            Group {
                name_hash,
                crc: reader.u32()?,
                version: 0,
                files: vec![],
            },
        );
    }
    for id in &ids {
        index.get_mut(id).context("Missing group metadata")?.version = reader.u32()?;
    }
    let counts = core::iter::repeat_with(|| reader.u16())
        .take(count)
        .collect::<Result<Vec<_>>>()?;
    for (&id, &count) in ids.iter().zip(&counts) {
        let mut file = 0_u32;
        for _ in 0..count {
            file = file
                .checked_add(reader.id(protocol)?)
                .context("File ID overflow")?;
            index
                .get_mut(&id)
                .context("Missing group metadata")?
                .files
                .push(file);
        }
    }
    if flags & 1 != 0 {
        for count in counts {
            let _reserved_names = reader.take(usize::from(count).saturating_mul(4))?;
        }
    }
    reader.finish()?;
    Ok(index)
}
/// Reassemble group files using signed chunk-table deltas.
#[allow(
    clippy::shadow_unrelated,
    reason = "each loop or conversion stage scopes its own intermediate values"
)]
pub(crate) fn split(data: &[u8], ids: &[u32]) -> Result<BTreeMap<u32, Vec<u8>>> {
    ensure!(!ids.is_empty(), "Empty group");
    if let &[id] = ids {
        return Ok(BTreeMap::from([(id, data.to_vec())]));
    }
    let chunks = usize::from(*data.last().context("Empty multi-file group")?);
    let table = chunks
        .checked_mul(ids.len())
        .and_then(|value| value.checked_mul(4))
        .context("Chunk table overflow")?;
    ensure!(data.len() > table, "Truncated chunk table");
    let end = data.len().checked_sub(1).context("Missing chunk count")?;
    let start = end.checked_sub(table).context("Truncated chunk table")?;
    let mut reader = Reader::new(data.get(start..end).context("Invalid chunk table range")?);
    let mut cursor = 0_usize;
    let mut result: BTreeMap<_, _> = ids.iter().map(|&id| (id, vec![])).collect();
    for _ in 0..chunks {
        let mut length = 0_i64;
        for id in ids {
            length = length
                .checked_add(i64::from(reader.i32()?))
                .context("Chunk length overflow")?;
            let count = usize::try_from(length).context("Invalid group chunk delta")?;
            let end = cursor.checked_add(count).context("Chunk offset overflow")?;
            ensure!(end <= start, "Chunk extends into file table");
            result
                .get_mut(id)
                .context("Missing group file")?
                .extend_from_slice(data.get(cursor..end).context("Invalid file chunk range")?);
            cursor = end;
        }
    }
    ensure!(cursor == start, "Unconsumed group payload");
    Ok(result)
}
#[cfg(test)]
#[allow(
    clippy::big_endian_bytes,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test fixtures use exact values, fixed indexes and panic on unexpected failures"
)]
mod tests {
    use super::*;
    #[test]
    fn split_signed_deltas() {
        let mut data = b"abcdef".to_vec();
        data.extend_from_slice(&4_i32.to_be_bytes());
        data.extend_from_slice(&(-2_i32).to_be_bytes());
        data.push(1);
        let parts = split(&data, &[0, 9]).unwrap();
        assert_eq!(parts[&0], b"abcd");
        assert_eq!(parts[&9], b"ef");
    }
    #[test]
    fn split_rejects_negative_and_out_of_payload_chunks() {
        for delta in [-1_i32, 2_i32] {
            let mut data = vec![42];
            data.extend_from_slice(&delta.to_be_bytes());
            data.extend_from_slice(&0_i32.to_be_bytes());
            data.push(1);
            let _invalid_chunk = split(&data, &[0, 1]).unwrap_err();
        }
    }
    #[test]
    #[allow(
        clippy::absolute_paths,
        reason = "qualified names identify the format or IO operation directly"
    )]
    fn compression_roundtrip() {
        use std::io::Write as _;
        let bytes = b"real native bzip2 validation";
        let mut encoder = bzip2::write::BzEncoder::new(vec![], bzip2::Compression::best());
        encoder.write_all(bytes).unwrap();
        let packed = encoder.finish().unwrap();
        let mut container = vec![1];
        container.extend_from_slice(&u32::try_from(packed.len() - 4).unwrap().to_be_bytes());
        container.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_be_bytes());
        container.extend_from_slice(&packed[4..]);
        assert_eq!(decode(&container).unwrap().0, bytes);
        let _error = decode(&container[..container.len() - 1]).unwrap_err();
    }
}
