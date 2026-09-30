//! text_to_docx (xuất bản dịch/tóm tắt của AI Assistant): ghi → đọc lại gói zip.

use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ff_ai_docx_{}_{name}", std::process::id()))
}

/// Tìm entry `name` trong zip STORE (method 0) do engine ghi; trả nội dung.
fn stored_entry(zip: &[u8], name: &str) -> Option<String> {
    let mut i = 0usize;
    while i + 30 <= zip.len() {
        if zip[i..i + 4] != [0x50, 0x4b, 0x03, 0x04] {
            break;
        }
        let method = u16::from_le_bytes([zip[i + 8], zip[i + 9]]);
        let size = u32::from_le_bytes([zip[i + 18], zip[i + 19], zip[i + 20], zip[i + 21]]) as usize;
        let nlen = u16::from_le_bytes([zip[i + 26], zip[i + 27]]) as usize;
        let xlen = u16::from_le_bytes([zip[i + 28], zip[i + 29]]) as usize;
        let fname = std::str::from_utf8(&zip[i + 30..i + 30 + nlen]).ok()?;
        let data_at = i + 30 + nlen + xlen;
        if fname == name {
            assert_eq!(method, 0, "entry phải STORE");
            return String::from_utf8(zip[data_at..data_at + size].to_vec()).ok();
        }
        i = data_at + size;
    }
    None
}

#[test]
fn text_to_docx_writes_readable_package() {
    let out = tmp("a.docx");
    let text = "# Bản dịch\n\nĐoạn **quan trọng** & <thẻ>\n---\n## Trang 2\nDòng cuối";
    ff_engine::text_to_docx(text, &out).expect("ghi docx");
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    let ct = stored_entry(&bytes, "[Content_Types].xml").expect("content types");
    assert!(ct.contains("wordprocessingml.document.main+xml"));
    assert!(stored_entry(&bytes, "_rels/.rels").is_some());
    let doc = stored_entry(&bytes, "word/document.xml").expect("document.xml");
    // Tiêu đề: đậm + cỡ 18pt; ký tự đặc biệt được escape; **x** → run đậm.
    assert!(doc.contains(r#"<w:b/><w:sz w:val="36"/>"#));
    assert!(doc.contains("Bản dịch"));
    assert!(doc.contains("&amp; &lt;thẻ&gt;"));
    assert!(doc.contains(r#"<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">quan trọng</w:t></w:r>"#));
    assert!(doc.contains(r#"<w:br w:type="page"/>"#));
    assert!(doc.contains("Dòng cuối"));
    assert!(!doc.contains("**"));
    let _ = std::fs::remove_file(out);
}
