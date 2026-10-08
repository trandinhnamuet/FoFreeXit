//! Định dạng gói runtime nhúng trong exe (pdfium.dll, qpdf, fonts/) cho bản
//! 1 file. Dùng chung 2 phía: `build.rs` ghi (`write_pack`), app đọc
//! (`read_index` + `unpack`, xem runtime.rs).
//!
//! Bố cục: MAGIC | u32 số entry | mỗi entry: u16 độ dài tên, tên UTF-8 (phân
//! cách '/'), u64 cỡ gốc, u64 cỡ nén | các khối deflate nối tiếp đúng thứ tự.
//! Mỗi file nén RIÊNG để lần chạy sau chỉ cần đọc mục lục (so cỡ file đã giải
//! nén), không phải giải nén lại cả gói.

use std::io::{self, Read, Write};

const MAGIC: &[u8; 8] = b"FFRT\x01\0\0\0";

pub struct Entry {
    pub name: String,
    pub size: u64,
    offset: usize,
    comp_len: usize,
}

pub fn write_pack(files: &[(String, Vec<u8>)]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let mut blobs = Vec::with_capacity(files.len());
    for (name, data) in files {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(data)?;
        let comp = enc.finish()?;
        let nb = name.as_bytes();
        out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        out.extend_from_slice(nb);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&(comp.len() as u64).to_le_bytes());
        blobs.push(comp);
    }
    for b in blobs {
        out.extend_from_slice(&b);
    }
    Ok(out)
}

fn take<'a>(p: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if p.len() < n {
        return None;
    }
    let (a, b) = p.split_at(n);
    *p = b;
    Some(a)
}

fn take_u64(p: &mut &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(take(p, 8)?.try_into().ok()?))
}

/// Mục lục gói; `None` nếu gói rỗng/hỏng.
pub fn read_index(pack: &[u8]) -> Option<Vec<Entry>> {
    let mut p = pack;
    if take(&mut p, MAGIC.len())? != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?) as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = u16::from_le_bytes(take(&mut p, 2)?.try_into().ok()?) as usize;
        let name = String::from_utf8(take(&mut p, name_len)?.to_vec()).ok()?;
        let size = take_u64(&mut p)?;
        let comp_len = usize::try_from(take_u64(&mut p)?).ok()?;
        entries.push(Entry { name, size, offset: 0, comp_len });
    }
    let mut offset = pack.len() - p.len();
    for e in &mut entries {
        e.offset = offset;
        offset = offset.checked_add(e.comp_len)?;
    }
    (offset == pack.len()).then_some(entries)
}

pub fn unpack(pack: &[u8], e: &Entry) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(e.size as usize);
    flate2::read::DeflateDecoder::new(&pack[e.offset..e.offset + e.comp_len]).read_to_end(&mut out)?;
    if out.len() as u64 != e.size {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{}: sai cỡ sau giải nén", e.name)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let files = vec![
            ("pdfium.dll".to_string(), vec![7u8; 10_000]),
            ("fonts/NotoSans-Regular.ttf".to_string(), b"font".to_vec()),
            ("empty.txt".to_string(), Vec::new()),
        ];
        let pack = write_pack(&files).unwrap();
        let idx = read_index(&pack).unwrap();
        assert_eq!(idx.len(), 3);
        for (e, (name, data)) in idx.iter().zip(&files) {
            assert_eq!(&e.name, name);
            assert_eq!(&unpack(&pack, e).unwrap(), data);
        }
        assert!(read_index(&pack[..pack.len() - 1]).is_none());
        assert!(read_index(&[]).is_none());
        assert_eq!(read_index(&write_pack(&[]).unwrap()).unwrap().len(), 0);
    }
}
