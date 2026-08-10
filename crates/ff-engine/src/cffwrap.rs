//! Parser CFF (Compact Font Format) TỐI THIỂU — phục vụ bọc font Type1C trần
//! (FontFile3 trong PDF, không có vỏ sfnt) thành OTF cho FontFace của WebView.
//!
//! Chỉ đọc những gì cần cho vỏ OTF: số glyph (CharStrings INDEX), unitsPerEm
//! (FontMatrix), mã→GID (Encoding tuỳ biến của font đơn giản) hoặc CID→GID
//! (charset của font CID). KHÔNG diễn giải charstring — glyph outline giữ
//! nguyên trong bảng `CFF ` của OTF, trình duyệt tự vẽ.

use std::collections::HashMap;

pub(crate) struct CffInfo {
    pub num_glyphs: u16,
    pub upm: f32,
    pub is_cid: bool,
    /// Font đơn giản: mã byte → GID (từ Encoding tuỳ biến format 0/1).
    pub code_to_gid: HashMap<u32, u16>,
    /// Font CID: CID → GID (nghịch đảo charset).
    pub cid_to_gid: HashMap<u32, u16>,
}

/// Đọc 1 INDEX tại `pos`: trả (vị trí kết thúc, các item). CFF INDEX: count
/// u16; count>0: offSize u8 + (count+1) offset + data (offset tính từ 1).
fn read_index(data: &[u8], pos: usize) -> Option<(usize, Vec<&[u8]>)> {
    let count = u16::from_be_bytes(data.get(pos..pos + 2)?.try_into().ok()?) as usize;
    if count == 0 {
        return Some((pos + 2, Vec::new()));
    }
    let off_size = *data.get(pos + 2)? as usize;
    if !(1..=4).contains(&off_size) {
        return None;
    }
    let offs_start = pos + 3;
    let read_off = |i: usize| -> Option<usize> {
        let s = offs_start + i * off_size;
        let b = data.get(s..s + off_size)?;
        Some(b.iter().fold(0usize, |a, &x| (a << 8) | x as usize))
    };
    let data_start = offs_start + (count + 1) * off_size - 1;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let a = data_start + read_off(i)?;
        let b = data_start + read_off(i + 1)?;
        if a > b || b > data.len() {
            return None;
        }
        items.push(&data[a..b]);
    }
    Some((data_start + read_off(count)?, items))
}

/// DICT CFF → map operator → operands. Escape (12 xx) mã hoá 0x0c00|xx.
fn parse_dict(d: &[u8]) -> HashMap<u16, Vec<f64>> {
    let mut out = HashMap::new();
    let mut operands: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let b0 = d[i];
        match b0 {
            32..=246 => {
                operands.push(f64::from(b0) - 139.0);
                i += 1;
            }
            247..=250 => {
                let b1 = f64::from(d.get(i + 1).copied().unwrap_or(0));
                operands.push((f64::from(b0) - 247.0) * 256.0 + b1 + 108.0);
                i += 2;
            }
            251..=254 => {
                let b1 = f64::from(d.get(i + 1).copied().unwrap_or(0));
                operands.push(-(f64::from(b0) - 251.0) * 256.0 - b1 - 108.0);
                i += 2;
            }
            28 => {
                let v = d
                    .get(i + 1..i + 3)
                    .map(|b| i16::from_be_bytes([b[0], b[1]]))
                    .unwrap_or(0);
                operands.push(f64::from(v));
                i += 3;
            }
            29 => {
                let v = d
                    .get(i + 1..i + 5)
                    .map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
                    .unwrap_or(0);
                operands.push(f64::from(v));
                i += 5;
            }
            30 => {
                // Real: nibble packed, kết thúc 0xf.
                let mut s = String::new();
                let mut j = i + 1;
                'outer: while j < d.len() {
                    for nib in [d[j] >> 4, d[j] & 0x0f] {
                        match nib {
                            0..=9 => s.push(char::from(b'0' + nib)),
                            0xa => s.push('.'),
                            0xb => s.push('E'),
                            0xc => s.push_str("E-"),
                            0xe => s.push('-'),
                            0xf => {
                                j += 1;
                                break 'outer;
                            }
                            _ => {}
                        }
                    }
                    j += 1;
                }
                operands.push(s.parse::<f64>().unwrap_or(0.0));
                i = j;
            }
            12 => {
                let op = 0x0c00 | u16::from(d.get(i + 1).copied().unwrap_or(0));
                out.insert(op, std::mem::take(&mut operands));
                i += 2;
            }
            0..=21 => {
                out.insert(u16::from(b0), std::mem::take(&mut operands));
                i += 1;
            }
            _ => break,
        }
    }
    out
}

pub(crate) fn parse(data: &[u8]) -> Option<CffInfo> {
    let hdr_size = *data.get(2)? as usize;
    if data.first() != Some(&1) || hdr_size < 4 {
        return None;
    }
    let (p1, _names) = read_index(data, hdr_size)?;
    let (p2, tops) = read_index(data, p1)?;
    let _ = read_index(data, p2)?; // String INDEX — không cần nội dung
    let top = parse_dict(tops.first()?);

    let charstrings_off = *top.get(&17)?.first()? as usize;
    let (_, charstrings) = read_index(data, charstrings_off)?;
    let num_glyphs = u16::try_from(charstrings.len()).ok()?;
    if num_glyphs == 0 {
        return None;
    }
    let is_cid = top.contains_key(&0x0c1e); // ROS

    // unitsPerEm từ FontMatrix (mặc định 0.001 → 1000).
    let upm = top
        .get(&0x0c07)
        .and_then(|m| m.first().copied())
        .filter(|&a| a > 1e-9)
        .map(|a| (1.0 / a) as f32)
        .unwrap_or(1000.0);

    let mut code_to_gid = HashMap::new();
    if !is_cid {
        // Encoding: 0/1 = Standard/Expert (định nghĩa theo TÊN glyph — cần
        // charset+String INDEX, ngoài phạm vi v1); offset = tuỳ biến fmt 0/1.
        let enc = top.get(&16).and_then(|v| v.first().copied()).unwrap_or(0.0) as usize;
        if enc > 1 && enc < data.len() {
            let fmt = data[enc] & 0x7f;
            if fmt == 0 {
                let n = *data.get(enc + 1)? as usize;
                for k in 0..n {
                    let code = *data.get(enc + 2 + k)? as u32;
                    code_to_gid.insert(code, k as u16 + 1);
                }
            } else if fmt == 1 {
                let n_ranges = *data.get(enc + 1)? as usize;
                let mut gid: u16 = 1;
                for r in 0..n_ranges {
                    let first = *data.get(enc + 2 + r * 2)? as u32;
                    let n_left = *data.get(enc + 3 + r * 2)? as u32;
                    for k in 0..=n_left {
                        if gid >= num_glyphs {
                            break;
                        }
                        code_to_gid.insert(first + k, gid);
                        gid += 1;
                    }
                }
            }
        }
    }

    let mut cid_to_gid = HashMap::new();
    if is_cid {
        // charset (op 15): gid → CID; nghịch đảo lại. 0/1/2 = bảng dựng sẵn
        // (ISOAdobe... — CID font thực tế luôn dùng offset tuỳ biến).
        let cs = top.get(&15).and_then(|v| v.first().copied()).unwrap_or(0.0) as usize;
        cid_to_gid.insert(0, 0);
        if cs > 2 && cs < data.len() {
            let fmt = data[cs];
            match fmt {
                0 => {
                    for gid in 1..num_glyphs {
                        let o = cs + 1 + (gid as usize - 1) * 2;
                        let cid =
                            u16::from_be_bytes(data.get(o..o + 2)?.try_into().ok()?) as u32;
                        cid_to_gid.insert(cid, gid);
                    }
                }
                1 | 2 => {
                    let step = if fmt == 1 { 3 } else { 4 };
                    let mut gid: u16 = 1;
                    let mut o = cs + 1;
                    while gid < num_glyphs {
                        let first =
                            u16::from_be_bytes(data.get(o..o + 2)?.try_into().ok()?) as u32;
                        let n_left = if fmt == 1 {
                            u32::from(*data.get(o + 2)?)
                        } else {
                            u32::from(u16::from_be_bytes(data.get(o + 2..o + 4)?.try_into().ok()?))
                        };
                        for k in 0..=n_left {
                            if gid >= num_glyphs {
                                break;
                            }
                            cid_to_gid.insert(first + k, gid);
                            gid += 1;
                        }
                        o += step;
                    }
                }
                _ => {}
            }
        } else if cs == 0 {
            // charset 0 với CID = Identity (hiếm): CID = GID.
            for gid in 0..num_glyphs {
                cid_to_gid.insert(u32::from(gid), gid);
            }
        }
    }

    Some(CffInfo { num_glyphs, upm, is_cid, code_to_gid, cid_to_gid })
}

/// CFF giả tối thiểu cho test: 3 glyph, encoding format 0 (mã 65 'A' → gid 1,
/// mã 66 'B' → gid 2), FontMatrix mặc định. Dùng chung với test fontfix.
#[cfg(test)]
pub(crate) fn fake_cff() -> Vec<u8> {
    build_fake_cff()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_cff_encoding_and_glyph_count() {
        let cff = fake_cff();
        let info = parse(&cff).expect("parse CFF");
        assert_eq!(info.num_glyphs, 3);
        assert!(!info.is_cid);
        assert_eq!(info.upm, 1000.0);
        assert_eq!(info.code_to_gid.get(&65), Some(&1), "'A' → gid 1");
        assert_eq!(info.code_to_gid.get(&66), Some(&2), "'B' → gid 2");
    }
}

#[cfg(test)]
fn build_fake_cff() -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(&[1, 0, 4, 1]); // header: major minor hdrSize offSize
        // Name INDEX: 1 item "T"
        v.extend_from_slice(&[0, 1, 1, 1, 2]); // count=1 offSize=1 offs [1,2]
        v.push(b'T');
        // Top DICT INDEX: 1 item (điền sau khi biết offset các phần)
        // DICT: charstrings offset (op 17) + encoding offset (op 16).
        // Offset tuyệt đối — tính trước bằng cách dựng phần đuôi trước.
        // Layout sau Top DICT INDEX: String INDEX (rỗng, 2B) + GSubr INDEX
        // (rỗng, 2B) + encoding + charstrings.
        // Top DICT nội dung: "29 <i32 cs_off> 17 29 <i32 enc_off> 16" = 12B.
        let dict_len = 12usize;
        let top_index_len = 2 + 1 + 2 + dict_len; // count offSize offs[2] data
        let after_top = v.len() + top_index_len;
        let enc_off = after_top + 2 + 2; // sau String INDEX + GSubr INDEX
        let enc_len = 2 + 2; // format0, nCodes=2, codes[65,66]
        let cs_off = enc_off + enc_len;
        // Top DICT INDEX header
        v.extend_from_slice(&[0, 1, 1, 1, (1 + dict_len) as u8]);
        // DICT bytes
        v.push(29);
        v.extend_from_slice(&(cs_off as i32).to_be_bytes());
        v.push(17);
        v.push(29);
        v.extend_from_slice(&(enc_off as i32).to_be_bytes());
        v.push(16);
        assert_eq!(v.len(), after_top);
        v.extend_from_slice(&[0, 0]); // String INDEX rỗng
        v.extend_from_slice(&[0, 0]); // GSubr INDEX rỗng
        // Encoding format 0
        assert_eq!(v.len(), enc_off);
        v.extend_from_slice(&[0, 2, 65, 66]);
        // CharStrings INDEX: 3 glyph 1 byte mỗi cái (nội dung không quan trọng)
        assert_eq!(v.len(), cs_off);
        v.extend_from_slice(&[0, 3, 1, 1, 2, 3, 4, 0x0e, 0x0e, 0x0e]);
        v
}
