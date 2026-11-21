//! A read-only ISO 9660 image with Joliet names (ECMA-119 plus the Joliet extension), written with
//! no external tool so the snapshot works the same on macOS, Linux and Windows.
//!
//! The image holds names and file contents only: no permissions, timestamps, links or device
//! nodes. Joliet carries the real names (up to 64 UTF-16 units). The ISO tree gets generated
//! Level-1 names for readers that ignore Joliet, and both trees point at the same file extents.
//! The layout is deterministic for a given file list, so the same input gives the same bytes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};

#[cfg(test)]
pub(super) mod tests;

const SECTOR: usize = 2048;
/// The first path table: after the 16-sector system area and the three volume descriptors.
const FIRST_TABLE_SECTOR: u64 = 19;
/// Joliet limits a name to 64 UTF-16 units.
const JOLIET_MAX_UNITS: usize = 64;
/// Fixed record date (1970-01-01 00:00:00, GMT+0) so the same input yields the same bytes.
const RECORD_DATE: [u8; 7] = [70, 1, 1, 0, 0, 0, 0];
const SYSTEM_ID: &str = "DIAL DESKTOP";
const VOLUME_ID: &str = "DIAL_PROJECT";
const APPLICATION_ID: &str = "DIAL DESKTOP PROJECT SNAPSHOT";
const STANDARD_ID: &[u8; 5] = b"CD001";

/// One regular file to place in the image. `path` uses `/` between components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ImageFile {
    pub(super) path: String,
    pub(super) size: u64,
}

/// Writes the image: volume descriptors, path tables, directories, then each file's bytes in
/// the order of `files`. `content(index, out)` must write exactly `files[index].size` bytes.
/// Returns the image length in bytes.
pub(super) fn write_image(
    files: &[ImageFile],
    content: &mut dyn FnMut(usize, &mut dyn Write) -> Result<(), String>,
    out: &mut dyn Write,
) -> Result<u64, String> {
    let tree = build_tree(files)?;
    // Index 0 is the ISO 9660 tree, index 1 the Joliet tree.
    let volumes = [
        flatten(&tree, Naming::Iso)?,
        flatten(&tree, Naming::Joliet)?,
    ];
    if volumes
        .iter()
        .any(|dirs| dirs.len() > usize::from(u16::MAX))
    {
        return Err("too_many_directories".to_owned());
    }
    let mut file_extents = Vec::with_capacity(files.len());
    for file in files {
        let bytes =
            u32::try_from(file.size).map_err(|_| format!("file_too_large: {}", file.path))?;
        file_extents.push(Extent { lba: 0, bytes });
    }

    // Pass one assigns extents. Record lengths depend only on names and counts, never on the
    // extent values, so zero extents give the sizes that pass two must reproduce.
    let mut next = FIRST_TABLE_SECTOR;
    let mut tables = [[Extent::default(); 2]; 2];
    for (volume, dirs) in volumes.iter().enumerate() {
        let placeholder = vec![Extent::default(); dirs.len()];
        for big in [false, true] {
            let len = path_table(dirs, &placeholder, big).len();
            tables[volume][usize::from(big)] = Extent {
                lba: next,
                bytes: to_u32(len as u64)?,
            };
            next += sectors(len as u64);
        }
    }
    let mut dir_extents: [Vec<Extent>; 2] = [Vec::new(), Vec::new()];
    for (volume, dirs) in volumes.iter().enumerate() {
        let placeholder = vec![Extent::default(); dirs.len()];
        for index in 0..dirs.len() {
            let len = pack(&directory_records(dirs, index, &placeholder, &file_extents)).len();
            dir_extents[volume].push(Extent {
                lba: next,
                bytes: to_u32(len as u64)?,
            });
            next += sectors(len as u64);
        }
    }
    let metadata_sectors = next;
    for extent in &mut file_extents {
        extent.lba = next;
        next += sectors(u64::from(extent.bytes));
    }
    if next > u64::from(u32::MAX) {
        return Err("image_too_large".to_owned());
    }

    // Pass two writes the metadata with the real extents.
    let total = to_u32(next)?;
    let mut image = vec![0u8; metadata_sectors as usize * SECTOR];
    let iso_root = record(dir_extents[0][0], true, &[0]);
    let joliet_root = record(dir_extents[1][0], true, &[0]);
    put(
        &mut image,
        16 * SECTOR,
        &descriptor(1, total, tables[0], &iso_root),
    )?;
    put(
        &mut image,
        17 * SECTOR,
        &descriptor(2, total, tables[1], &joliet_root),
    )?;
    put(&mut image, 18 * SECTOR, &terminator())?;
    for (volume, dirs) in volumes.iter().enumerate() {
        for big in [false, true] {
            let table = path_table(dirs, &dir_extents[volume], big);
            let extent = tables[volume][usize::from(big)];
            put(&mut image, extent.lba as usize * SECTOR, &table)?;
        }
        for (index, extent) in dir_extents[volume].iter().enumerate() {
            let body = pack(&directory_records(
                dirs,
                index,
                &dir_extents[volume],
                &file_extents,
            ));
            if body.len() != extent.bytes as usize {
                return Err("internal_layout: directory size changed".to_owned());
            }
            put(&mut image, extent.lba as usize * SECTOR, &body)?;
        }
    }
    out.write_all(&image).map_err(io_failed)?;

    for (index, file) in files.iter().enumerate() {
        {
            let mut counted = Counted {
                inner: &mut *out,
                written: 0,
                limit: file.size,
            };
            content(index, &mut counted)?;
            if counted.written != file.size {
                return Err(format!("content_size_mismatch: {}", file.path));
            }
        }
        let pad = (sectors(file.size) * SECTOR as u64 - file.size) as usize;
        out.write_all(&vec![0u8; pad]).map_err(io_failed)?;
    }
    Ok(next * SECTOR as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Naming {
    Iso,
    Joliet,
}

/// A directory or file in the input tree. Files carry their index into the input list.
enum Node {
    Dir(BTreeMap<String, Node>),
    File(usize),
}

/// ECMA-119 limits the directory hierarchy to eight levels, the root included.
const MAX_PATH_COMPONENTS: usize = 8;
/// The Joliet specification caps a whole path, separators included, at 240 characters.
const JOLIET_MAX_PATH_UNITS: usize = 240;

fn build_tree(files: &[ImageFile]) -> Result<BTreeMap<String, Node>, String> {
    let mut root = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        let parts: Vec<&str> = file.path.split('/').collect();
        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains('\0'))
        {
            return Err(format!("invalid_path: {}", file.path));
        }
        // A file may have at most eight path components (the root is level one).
        if parts.len() > MAX_PATH_COMPONENTS {
            return Err(format!(
                "too_deep: {} has {} components",
                file.path,
                parts.len()
            ));
        }
        // Joliet caps a whole path at 240 UTF-16 units.
        if file.path.encode_utf16().count() > JOLIET_MAX_PATH_UNITS {
            return Err(format!("path_too_long: {}", file.path));
        }
        let Some((last, dirs)) = parts.split_last() else {
            return Err(format!("invalid_path: {}", file.path));
        };
        let mut current = &mut root;
        for dir in dirs {
            let node = current
                .entry((*dir).to_owned())
                .or_insert_with(|| Node::Dir(BTreeMap::new()));
            current = match node {
                Node::Dir(children) => children,
                Node::File(_) => return Err(format!("path_conflict: {}", file.path)),
            };
        }
        if current.contains_key(*last) {
            return Err(format!("duplicate_path: {}", file.path));
        }
        current.insert((*last).to_owned(), Node::File(index));
    }
    Ok(root)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Dir(usize),
    File(usize),
}

struct Child {
    id: Vec<u8>,
    target: Target,
}

/// One directory of a volume. Directory 0 is the root; `parent` of the root is 0.
struct DirPlan {
    parent: usize,
    own_id: Vec<u8>,
    children: Vec<Child>,
}

/// Breadth-first, so the directory numbers match the path-table order.
fn flatten(root: &BTreeMap<String, Node>, naming: Naming) -> Result<Vec<DirPlan>, String> {
    let mut dirs = vec![DirPlan {
        parent: 0,
        own_id: vec![0],
        children: Vec::new(),
    }];
    let mut queue: VecDeque<(usize, &BTreeMap<String, Node>)> = VecDeque::from([(0, root)]);
    while let Some((index, entries)) = queue.pop_front() {
        let names: Vec<(&str, bool)> = entries
            .iter()
            .map(|(name, node)| (name.as_str(), matches!(node, Node::Dir(_))))
            .collect();
        let ids = match naming {
            Naming::Iso => iso_identifiers(&names),
            Naming::Joliet => joliet_identifiers(&names)?,
        };
        for ((_, node), id) in entries.iter().zip(ids) {
            let target = match node {
                Node::File(file) => Target::File(*file),
                Node::Dir(children) => {
                    let child = dirs.len();
                    dirs.push(DirPlan {
                        parent: index,
                        own_id: id.clone(),
                        children: Vec::new(),
                    });
                    queue.push_back((child, children));
                    Target::Dir(child)
                }
            };
            dirs[index].children.push(Child { id, target });
        }
    }
    Ok(dirs)
}

/// Level-1 identifiers: `STEM.EXT;1` for files, `STEM` for directories. Characters outside
/// A-Z, 0-9 and `_` become `_`; a clash inside one directory gets a `_N` suffix.
fn iso_identifiers(entries: &[(&str, bool)]) -> Vec<Vec<u8>> {
    let mut used = BTreeSet::new();
    entries
        .iter()
        .map(|&(name, is_dir)| {
            let (stem, ext) = split_name(name, is_dir);
            let stem = match sanitize(stem, 8) {
                s if s.is_empty() => "_".to_owned(),
                s => s,
            };
            let ext = ext.map(|e| sanitize(e, 3)).filter(|e| !e.is_empty());
            let mut n = 0usize;
            let key = loop {
                let suffix = if n == 0 {
                    String::new()
                } else {
                    format!("_{n}")
                };
                let head: String = stem.chars().take(8 - suffix.len()).collect();
                let key = match &ext {
                    Some(e) => format!("{head}{suffix}.{e}"),
                    None => format!("{head}{suffix}"),
                };
                if used.insert(key.clone()) {
                    break key;
                }
                n += 1;
            };
            if is_dir {
                key.into_bytes()
            } else {
                format!("{key};1").into_bytes()
            }
        })
        .collect()
}

fn split_name(name: &str, is_dir: bool) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(dot) if !is_dir && dot > 0 => (&name[..dot], Some(&name[dot + 1..])),
        _ => (name, None),
    }
}

fn sanitize(text: &str, max: usize) -> String {
    text.chars()
        .take(max)
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Joliet identifiers are UTF-16BE. Joliet forbids control characters and `* / : ; ? \`, so
/// those names are refused rather than altered.
fn joliet_identifiers(entries: &[(&str, bool)]) -> Result<Vec<Vec<u8>>, String> {
    entries
        .iter()
        .map(|&(name, _)| {
            if name
                .chars()
                .any(|c| c.is_control() || "*/:;?\\".contains(c))
            {
                return Err(format!("unsupported_name: {name:?}"));
            }
            let units: Vec<u16> = name.encode_utf16().collect();
            if units.len() > JOLIET_MAX_UNITS {
                return Err(format!("name_too_long: {name}"));
            }
            Ok(units.iter().flat_map(|unit| unit.to_be_bytes()).collect())
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct Extent {
    lba: u64,
    bytes: u32,
}

/// One directory record (ECMA-119 section 9.1).
fn record(extent: Extent, is_dir: bool, id: &[u8]) -> Vec<u8> {
    let len = 33 + id.len() + usize::from(id.len().is_multiple_of(2));
    let mut out = vec![0u8; len];
    out[0] = len as u8;
    both32(&mut out, 2, extent.lba as u32);
    both32(&mut out, 10, extent.bytes);
    out[18..25].copy_from_slice(&RECORD_DATE);
    out[25] = if is_dir { 2 } else { 0 };
    both16(&mut out, 28, 1);
    out[32] = id.len() as u8;
    out[33..33 + id.len()].copy_from_slice(id);
    out
}

fn directory_records(
    dirs: &[DirPlan],
    index: usize,
    dir_extents: &[Extent],
    file_extents: &[Extent],
) -> Vec<Vec<u8>> {
    let dir = &dirs[index];
    let mut records = vec![
        record(dir_extents[index], true, &[0]),
        record(dir_extents[dir.parent], true, &[1]),
    ];
    for child in &dir.children {
        records.push(match child.target {
            Target::Dir(target) => record(dir_extents[target], true, &child.id),
            Target::File(file) => record(file_extents[file], false, &child.id),
        });
    }
    records
}

/// Packs records so none crosses a sector boundary (ECMA-119 section 6.8.1.1); the rest of a
/// sector is zero, which readers treat as the end of that sector's records.
fn pack(records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for record in records {
        if body.len() % SECTOR + record.len() > SECTOR {
            body.resize(body.len().next_multiple_of(SECTOR), 0);
        }
        body.extend_from_slice(record);
    }
    body.resize(body.len().next_multiple_of(SECTOR), 0);
    body
}

/// The L (little-endian) or M (big-endian) path table (ECMA-119 section 9.4).
fn path_table(dirs: &[DirPlan], extents: &[Extent], big: bool) -> Vec<u8> {
    let mut table = Vec::new();
    for (dir, extent) in dirs.iter().zip(extents) {
        let id = &dir.own_id;
        let lba = extent.lba as u32;
        let parent = (dir.parent + 1) as u16;
        table.push(id.len() as u8);
        table.push(0);
        table.extend_from_slice(&if big {
            lba.to_be_bytes()
        } else {
            lba.to_le_bytes()
        });
        table.extend_from_slice(&if big {
            parent.to_be_bytes()
        } else {
            parent.to_le_bytes()
        });
        table.extend_from_slice(id);
        if !id.len().is_multiple_of(2) {
            table.push(0);
        }
    }
    table
}

/// A primary (`kind` 1) or Joliet supplementary (`kind` 2) volume descriptor (section 8.4).
/// `tables` holds the L and M path table extents of that volume.
fn descriptor(kind: u8, total: u32, tables: [Extent; 2], root: &[u8]) -> Vec<u8> {
    let mut d = vec![0u8; SECTOR];
    d[0] = kind;
    d[1..6].copy_from_slice(STANDARD_ID);
    d[6] = 1;
    if kind == 2 {
        // Escape sequence %/E selects UCS-2 Level 3, which marks the tree as Joliet.
        d[88..91].copy_from_slice(b"%/E");
    }
    fill(&mut d, 8, 32, SYSTEM_ID);
    fill(&mut d, 40, 32, VOLUME_ID);
    both32(&mut d, 80, total);
    both16(&mut d, 120, 1);
    both16(&mut d, 124, 1);
    both16(&mut d, 128, SECTOR as u16);
    both32(&mut d, 132, tables[0].bytes);
    d[140..144].copy_from_slice(&(tables[0].lba as u32).to_le_bytes());
    d[148..152].copy_from_slice(&(tables[1].lba as u32).to_be_bytes());
    d[156..190].copy_from_slice(root);
    fill(&mut d, 190, 128, "");
    fill(&mut d, 318, 128, "");
    fill(&mut d, 446, 128, "");
    fill(&mut d, 574, 128, APPLICATION_ID);
    fill(&mut d, 702, 37, "");
    fill(&mut d, 739, 37, "");
    fill(&mut d, 776, 37, "");
    // Creation, modification and effective dates are fixed; expiration is "never" (zeros).
    for at in [813, 830, 864] {
        d[at..at + 16].copy_from_slice(b"1970010100000000");
    }
    d[847..863].copy_from_slice(b"0000000000000000");
    d[881] = 1;
    d
}

fn terminator() -> Vec<u8> {
    let mut d = vec![0u8; SECTOR];
    d[0] = 255;
    d[1..6].copy_from_slice(STANDARD_ID);
    d[6] = 1;
    d
}

/// Writes `text` into `d[at..at + len]`, padded with spaces as the standard requires.
fn fill(d: &mut [u8], at: usize, len: usize, text: &str) {
    for (i, slot) in d[at..at + len].iter_mut().enumerate() {
        *slot = text.as_bytes().get(i).copied().unwrap_or(b' ');
    }
}

fn both32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    buf[at + 4..at + 8].copy_from_slice(&value.to_be_bytes());
}

fn both16(buf: &mut [u8], at: usize, value: u16) {
    buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    buf[at + 2..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn put(image: &mut [u8], at: usize, bytes: &[u8]) -> Result<(), String> {
    let slot = image
        .get_mut(at..at + bytes.len())
        .ok_or_else(|| "internal_layout: write outside the image".to_owned())?;
    slot.copy_from_slice(bytes);
    Ok(())
}

fn sectors(bytes: u64) -> u64 {
    bytes.div_ceil(SECTOR as u64)
}

fn to_u32(value: u64) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "image_too_large".to_owned())
}

fn io_failed(error: io::Error) -> String {
    format!("io_failed: {error}")
}

/// Passes writes through and refuses more bytes than the file declared.
struct Counted<'a> {
    inner: &'a mut dyn Write,
    written: u64,
    limit: u64,
}

impl Write for Counted<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > self.limit {
            return Err(io::Error::other("content is longer than the declared size"));
        }
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
