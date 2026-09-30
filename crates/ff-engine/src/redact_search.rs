//! Search & Redact (Foxit: Protect › Redact › Search & Redact): tìm trên TOÀN
//! tài liệu theo từ/cụm từ, biểu thức chính quy, hoặc mẫu dựng sẵn (email, số
//! điện thoại VN, CCCD/CMND, mã số thuế, số thẻ ngân hàng — kiểm Luhn, IBAN —
//! kiểm mod 97, ngày tháng, URL, IPv4). Mỗi kết quả trả về các hộp (điểm PDF)
//! theo từng dòng — cụm khớp vắt qua nhiều dòng thành nhiều hộp — để UI đánh
//! dấu redact rồi đi tiếp luồng Áp dụng sẵn có.

use std::path::Path;

use pdfium_render::prelude::*;
use regex::{Regex, RegexBuilder};

use crate::text::Rect;
use crate::EngineError;

/// Mẫu dựng sẵn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactPattern {
    Email,
    /// Di động VN (0/+84 + đầu số 3/5/7/8/9) + cố định (02x…).
    Phone,
    /// Căn cước công dân 12 số.
    Cccd,
    /// Chứng minh nhân dân 9 số.
    Cmnd,
    /// Mã số thuế 10 số hoặc 13 số (10-3).
    TaxCode,
    /// Số thẻ ngân hàng 13–19 số, kiểm Luhn.
    BankCard,
    Iban,
    Date,
    Url,
    Ipv4,
}

impl RedactPattern {
    pub fn from_key(s: &str) -> Option<Self> {
        Some(match s {
            "email" => Self::Email,
            "phone" => Self::Phone,
            "cccd" => Self::Cccd,
            "cmnd" => Self::Cmnd,
            "taxCode" => Self::TaxCode,
            "bankCard" => Self::BankCard,
            "iban" => Self::Iban,
            "date" => Self::Date,
            "url" => Self::Url,
            "ipv4" => Self::Ipv4,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Cccd => "cccd",
            Self::Cmnd => "cmnd",
            Self::TaxCode => "taxCode",
            Self::BankCard => "bankCard",
            Self::Iban => "iban",
            Self::Date => "date",
            Self::Url => "url",
            Self::Ipv4 => "ipv4",
        }
    }
    fn regex(self) -> &'static str {
        match self {
            Self::Email => r"[\p{L}\p{N}][\p{L}\p{N}._%+\-]*@[\p{L}\p{N}](?:[\p{L}\p{N}\-]*[\p{L}\p{N}])?(?:\.[\p{L}\p{N}](?:[\p{L}\p{N}\-]*[\p{L}\p{N}])?)*\.\p{L}{2,}",
            // Di động: 0 hoặc +84/84 + 9 số (đầu 3/5/7/8/9), cho phép nhóm cách
            // bằng dấu cách/chấm/gạch. Cố định: 0 + 2x(x) + 7–8 số.
            Self::Phone => concat!(
                r"(?:\+84|0084|84)[ .\-]?\(?0?\)?[ .\-]?[35789]\d(?:[ .\-]?\d){7}",
                r"|\(?0[35789]\d\)?(?:[ .\-]?\d){7}",
                r"|(?:\+84[ .\-]?|\(?0)2\d{1,2}\)?[ .\-]?\d{3,4}[ .\-]?\d{3,4}",
            ),
            Self::Cccd => r"\d{3}[ .]?\d{3}[ .]?\d{6}|\d{12}",
            Self::Cmnd => r"\d{9}",
            Self::TaxCode => r"\d{10}[ ]?-[ ]?\d{3}|\d{13}|\d{10}",
            Self::BankCard => r"\d(?:[ \-]?\d){12,18}",
            Self::Iban => r"[A-Z]{2}\d{2}(?:[ ]?[A-Z0-9]){11,30}",
            Self::Date => concat!(
                r"(?i:ngày\s+\d{1,2}\s+tháng\s+\d{1,2}\s+năm\s+\d{4})",
                r"|\d{4}[\-/.]\d{1,2}[\-/.]\d{1,2}",
                r"|\d{1,2}[\-/.]\d{1,2}[\-/.](?:\d{4}|\d{2})",
                r"|(?i:\d{1,2}\s+(?:jan|feb|mar|apr|may|jun|jul|aug|sep|sept|oct|nov|dec)[a-z]*\.?,?\s+\d{4})",
                r"|(?i:(?:jan|feb|mar|apr|may|jun|jul|aug|sep|sept|oct|nov|dec)[a-z]*\.?\s+\d{1,2},?\s+\d{4})",
            ),
            Self::Url => r"(?i:(?:https?|ftp)://[^\s<>\x22]+|www\.[^\s<>\x22]+)",
            Self::Ipv4 => r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
        }
    }
    /// Mẫu số: không được dính liền chữ số khác ở hai đầu (thay lookaround).
    fn numeric(self) -> bool {
        matches!(
            self,
            Self::Phone | Self::Cccd | Self::Cmnd | Self::TaxCode | Self::BankCard | Self::Date | Self::Ipv4
        )
    }
    /// Kiểm tra ngữ nghĩa sau khi khớp regex.
    fn validate(self, m: &str) -> bool {
        match self {
            Self::BankCard => luhn_valid(m),
            Self::Iban => iban_valid(m),
            Self::Ipv4 => m.split('.').all(|p| p.parse::<u16>().map(|v| v <= 255).unwrap_or(false)),
            Self::Date => date_plausible(m),
            _ => true,
        }
    }
}

/// Tiêu chí tìm.
#[derive(Debug, Clone, Default)]
pub struct RedactSearchSpec {
    /// Từ/cụm từ (mỗi mục một cụm).
    pub terms: Vec<String>,
    /// Biểu thức chính quy do người dùng nhập.
    pub regex: Option<String>,
    pub patterns: Vec<RedactPattern>,
    pub match_case: bool,
    pub whole_word: bool,
    /// Giới hạn trang (0-based); rỗng = mọi trang.
    pub pages: Vec<u16>,
}

/// Một kết quả.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactHit {
    pub page_index: u16,
    /// Chuỗi khớp.
    pub text: String,
    /// Ngữ cảnh (…trước [khớp] sau…) để hiển thị.
    pub before: String,
    pub after: String,
    /// "term" | "regex" | key của mẫu.
    pub source: String,
    /// Hộp theo từng dòng (điểm PDF).
    pub rects: Vec<Rect>,
}

/// Thuật toán Luhn trên các chữ số của chuỗi (bỏ cách/gạch), 13–19 số.
pub fn luhn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) || s.chars().any(|c| !(c.is_ascii_digit() || c == ' ' || c == '-')) {
        return false;
    }
    // Loại chuỗi toàn một chữ số (0000…) — Luhn hợp lệ nhưng không phải số thẻ.
    if digits.iter().all(|&d| d == digits[0]) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

/// IBAN: chuyển 4 ký tự đầu xuống cuối, chữ → số (A=10…), mod 97 == 1.
pub fn iban_valid(s: &str) -> bool {
    let c: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if c.len() < 15 || c.len() > 34 {
        return false;
    }
    let (head, tail) = c.split_at(4);
    let mut rem: u32 = 0;
    for ch in tail.chars().chain(head.chars()) {
        let v = match ch {
            '0'..='9' => ch as u32 - '0' as u32,
            'A'..='Z' => ch as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        let digits = if v >= 10 { 2 } else { 1 };
        rem = (rem * if digits == 2 { 100 } else { 10 } + v) % 97;
    }
    rem == 1
}

fn date_plausible(m: &str) -> bool {
    let nums: Vec<u32> = m
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() == 3 {
        let (d, mo) = if nums[0] > 31 { (nums[2], nums[1]) } else { (nums[0], nums[1]) };
        return (1..=31).contains(&d) && (1..=12).contains(&mo);
    }
    // Dạng có tên tháng: chỉ cần ngày hợp lệ.
    nums.first().map(|&d| (1..=31).contains(&d)).unwrap_or(false)
}

/// Ký tự thuộc "từ" (chữ/số/dấu nối) — dùng cho khớp nguyên từ.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Một ký tự trên trang: vị trí byte trong `hay` + hộp.
struct PageChar {
    byte: usize,
    ch: char,
    rect: Option<Rect>,
}

/// Dựng text trang + bảng ký tự (thứ tự đọc của PDFium). Hộp dùng
/// `loose_bounds` (cao đủ ascent/descent — phủ trọn dòng khi bôi đen).
fn page_chars(text: &PdfPageText) -> (String, Vec<PageChar>) {
    let mut hay = String::new();
    let mut chars = Vec::new();
    for ch in text.chars().iter() {
        let c = ch.unicode_char().unwrap_or(' ');
        // Ký tự sinh (xuống dòng/cách) có thể không có hộp.
        let rect = ch
            .loose_bounds()
            .or_else(|_| ch.tight_bounds())
            .ok()
            .map(|b| Rect { left: b.left().value, bottom: b.bottom().value, right: b.right().value, top: b.top().value })
            .filter(|r| r.right > r.left && r.top > r.bottom);
        let c = if c == '\u{0}' || c == '\u{FFFE}' { ' ' } else { c };
        // Khoảng trắng lạ (NBSP, thin space…) → dấu cách thường, và gộp nhiều dấu
        // cách liền nhau thành một: tuỳ font/bản PDFium (Windows vs Linux) mà
        // "0912 345 678" ra NBSP hoặc 2 dấu cách → mẫu SĐT/số thẻ trượt.
        let c = if c != '\r' && c != '\n' && c.is_whitespace() { ' ' } else { c };
        if c == ' ' && hay.ends_with(' ') {
            continue;
        }
        chars.push(PageChar { byte: hay.len(), ch: c, rect });
        hay.push(c);
    }
    (hay, chars)
}

/// Gom hộp các ký tự [a, b) thành hộp theo dòng.
fn line_rects(chars: &[PageChar]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for pc in chars {
        if pc.ch == '\r' || pc.ch == '\n' {
            continue;
        }
        let Some(r) = pc.rect else { continue };
        if pc.ch.is_whitespace() && out.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut() {
            let h = (last.top - last.bottom).min(r.top - r.bottom).max(0.1);
            let overlap = last.top.min(r.top) - last.bottom.max(r.bottom);
            // Cùng dòng: chồng dọc ≥ 50% và không nhảy lùi quá xa.
            if overlap >= 0.5 * h && r.left >= last.left - h {
                last.left = last.left.min(r.left);
                last.right = last.right.max(r.right);
                last.bottom = last.bottom.min(r.bottom);
                last.top = last.top.max(r.top);
                continue;
            }
        }
        if pc.ch.is_whitespace() {
            continue; // dấu cách đầu dòng mới
        }
        out.push(r);
    }
    out
}

struct Matcher {
    re: Regex,
    source: String,
    pattern: Option<RedactPattern>,
    whole_word: bool,
}

fn build_matchers(spec: &RedactSearchSpec) -> Result<Vec<Matcher>, EngineError> {
    let mut out = Vec::new();
    let terms: Vec<&str> = spec.terms.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if !terms.is_empty() {
        // Dài trước để cụm dài thắng cụm con; khoảng trắng trong cụm khớp mọi
        // khoảng trắng (kể cả xuống dòng — cụm vắt dòng).
        let mut sorted = terms.clone();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
        let alts: Vec<String> = sorted
            .iter()
            .map(|t| t.split_whitespace().map(regex::escape).collect::<Vec<_>>().join(r"\s+"))
            .collect();
        let re = RegexBuilder::new(&alts.join("|"))
            .case_insensitive(!spec.match_case)
            .build()
            .map_err(|e| EngineError::Pdfium(format!("cụm từ tìm không hợp lệ: {e}")))?;
        out.push(Matcher { re, source: "term".into(), pattern: None, whole_word: spec.whole_word });
    }
    if let Some(r) = spec.regex.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let re = RegexBuilder::new(r)
            .case_insensitive(!spec.match_case)
            .size_limit(1 << 22)
            .build()
            .map_err(|e| EngineError::Pdfium(format!("biểu thức chính quy không hợp lệ: {e}")))?;
        out.push(Matcher { re, source: "regex".into(), pattern: None, whole_word: spec.whole_word });
    }
    for &p in &spec.patterns {
        let re = Regex::new(p.regex()).map_err(|e| EngineError::Pdfium(format!("mẫu {}: {e}", p.key())))?;
        out.push(Matcher { re, source: p.key().into(), pattern: Some(p), whole_word: false });
    }
    Ok(out)
}

/// Tìm các cụm khớp trong một chuỗi text trang: trả (byte_start, byte_end, source).
fn find_in_text(hay: &str, matchers: &[Matcher]) -> Vec<(usize, usize, String)> {
    let mut found: Vec<(usize, usize, String)> = Vec::new();
    for m in matchers {
        for mt in m.re.find_iter(hay) {
            let (mut s, mut e) = (mt.start(), mt.end());
            if s == e {
                continue;
            }
            let prev = hay[..s].chars().next_back();
            let next = hay[e..].chars().next();
            if let Some(p) = m.pattern {
                if p.numeric() {
                    // Không dính chữ số / chữ cái ở hai đầu (tránh cắt giữa số dài).
                    if prev.map(|c| c.is_ascii_digit() || (c.is_alphabetic() && p != RedactPattern::Date)).unwrap_or(false)
                        || next.map(|c| c.is_ascii_digit()).unwrap_or(false)
                    {
                        continue;
                    }
                    // Số cố định dạng "(0" mở ngoặc — giữ nguyên.
                }
                if p == RedactPattern::Url || p == RedactPattern::Email {
                    // Bỏ dấu câu cuối (".", ",", ")"…) dính theo.
                    while e > s {
                        let c = hay[..e].chars().next_back().unwrap();
                        if matches!(c, '.' | ',' | ';' | ':' | ')' | ']' | '!' | '?' | '\'' | '"') {
                            e -= c.len_utf8();
                        } else {
                            break;
                        }
                    }
                }
                if p == RedactPattern::Iban && prev.map(|c| c.is_alphanumeric()).unwrap_or(false) {
                    continue;
                }
                if !p.validate(&hay[s..e]) {
                    continue;
                }
            } else if m.whole_word {
                let first = hay[s..e].chars().next().unwrap();
                let last = hay[..e].chars().next_back().unwrap();
                if (is_word(first) && prev.map(is_word).unwrap_or(false))
                    || (is_word(last) && next.map(is_word).unwrap_or(false))
                {
                    continue;
                }
            }
            // Bỏ khoảng trắng hai đầu (regex người dùng có thể bắt dính).
            while s < e && hay[s..].starts_with(char::is_whitespace) {
                s += hay[s..].chars().next().unwrap().len_utf8();
            }
            while e > s && hay[..e].ends_with(char::is_whitespace) {
                e -= hay[..e].chars().next_back().unwrap().len_utf8();
            }
            if s < e {
                found.push((s, e, m.source.clone()));
            }
        }
    }
    // Khử trùng: bỏ kết quả nằm TRỌN trong kết quả khác (giữ cái dài hơn / đến trước).
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut kept: Vec<(usize, usize, String)> = Vec::new();
    for f in found {
        if kept.iter().any(|k| k.0 <= f.0 && f.1 <= k.1) {
            continue;
        }
        kept.push(f);
    }
    kept
}

/// Tìm trên toàn tài liệu. Kết quả theo thứ tự trang → vị trí.
pub fn search_redact(
    pdfium: &Pdfium,
    input: &Path,
    spec: &RedactSearchSpec,
    password: Option<&str>,
) -> Result<Vec<RedactHit>, EngineError> {
    let matchers = build_matchers(spec)?;
    if matchers.is_empty() {
        return Ok(Vec::new());
    }
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let mut hits = Vec::new();
    for (pi, page) in document.pages().iter().enumerate() {
        let pi = pi as u16;
        if !spec.pages.is_empty() && !spec.pages.contains(&pi) {
            continue;
        }
        let Ok(text) = page.text() else { continue };
        let (hay, chars) = page_chars(&text);
        if hay.trim().is_empty() {
            continue;
        }
        for (s, e, source) in find_in_text(&hay, &matchers) {
            let a = chars.partition_point(|c| c.byte < s);
            let b = chars.partition_point(|c| c.byte < e);
            let rects = line_rects(&chars[a..b]);
            if rects.is_empty() {
                continue;
            }
            let clean = |x: &str| x.replace(['\r', '\n'], " ");
            let before: String = {
                let v: Vec<char> = hay[..s].chars().collect();
                clean(&v[v.len().saturating_sub(32)..].iter().collect::<String>())
            };
            let after: String = clean(&hay[e..].chars().take(32).collect::<String>());
            hits.push(RedactHit {
                page_index: pi,
                text: clean(&hay[s..e]),
                before,
                after,
                source,
                rects,
            });
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(hay: &str, spec: RedactSearchSpec) -> Vec<String> {
        let m = build_matchers(&spec).unwrap();
        find_in_text(hay, &m).into_iter().map(|(s, e, _)| hay[s..e].to_string()).collect()
    }
    fn pat(p: RedactPattern) -> RedactSearchSpec {
        RedactSearchSpec { patterns: vec![p], ..Default::default() }
    }

    #[test]
    fn luhn_and_iban() {
        assert!(luhn_valid("4111 1111 1111 1111"));
        assert!(luhn_valid("5500-0000-0000-0004"));
        assert!(!luhn_valid("4111 1111 1111 1112"));
        assert!(iban_valid("GB82 WEST 1234 5698 7654 32"));
        assert!(!iban_valid("GB82 WEST 1234 5698 7654 33"));
    }

    #[test]
    fn vietnamese_patterns() {
        let t = "Liên hệ: nguyễn.văn@ví-dụ.vn hoặc an.nguyen@example.com.vn, ĐT 0912 345 678, +84 987654321, máy bàn 024 3826 1234.";
        assert_eq!(run(t, pat(RedactPattern::Email)), vec!["nguyễn.văn@ví-dụ.vn", "an.nguyen@example.com.vn"]);
        assert_eq!(run(t, pat(RedactPattern::Phone)), vec!["0912 345 678", "+84 987654321", "024 3826 1234"]);
        let id = "Số CCCD: 001203004567, CMND 123456789, MST 0101234567-001; số dài 1234567890123456789012";
        assert_eq!(run(id, pat(RedactPattern::Cccd)), vec!["001203004567"]);
        assert_eq!(run(id, pat(RedactPattern::Cmnd)), vec!["123456789"]);
        assert_eq!(run(id, pat(RedactPattern::TaxCode)), vec!["0101234567-001"]);
        let card = "Thẻ 4111 1111 1111 1111 và 4111 1111 1111 1112";
        assert_eq!(run(card, pat(RedactPattern::BankCard)), vec!["4111 1111 1111 1111"]);
        let d = "Hà Nội, ngày 5 tháng 9 năm 2024; hạn 31/12/2025, ISO 2024-02-29, sai 45/13/2020";
        assert_eq!(run(d, pat(RedactPattern::Date)), vec!["ngày 5 tháng 9 năm 2024", "31/12/2025", "2024-02-29"]);
        let u = "Xem https://ví-dụ.vn/trang?a=1. hoặc www.example.com, IP 192.168.1.10 và 999.1.1.1";
        assert_eq!(run(u, pat(RedactPattern::Url)), vec!["https://ví-dụ.vn/trang?a=1", "www.example.com"]);
        assert_eq!(run(u, pat(RedactPattern::Ipv4)), vec!["192.168.1.10"]);
    }

    #[test]
    fn terms_case_and_whole_word() {
        let t = "Nguyễn Văn An gặp NGUYỄN VĂN AN; Annam không phải An.";
        let s = |mc, ww| RedactSearchSpec { terms: vec!["nguyễn văn an".into()], match_case: mc, whole_word: ww, ..Default::default() };
        assert_eq!(run(t, s(false, false)).len(), 2);
        assert_eq!(run(t, s(true, false)).len(), 0);
        let w = |ww| RedactSearchSpec { terms: vec!["An".into()], match_case: true, whole_word: ww, ..Default::default() };
        assert_eq!(run(t, w(true)), vec!["An", "An"]);
        assert_eq!(run(t, w(false)).len(), 3);
        // Cụm vắt dòng: khoảng trắng khớp cả xuống dòng.
        let ml = "ông Nguyễn\r\nVăn An";
        assert_eq!(run(ml, s(false, false)), vec!["Nguyễn\r\nVăn An"]);
    }

    #[test]
    fn bad_regex_is_error() {
        let spec = RedactSearchSpec { regex: Some("(abc".into()), ..Default::default() };
        assert!(build_matchers(&spec).is_err());
    }
}
