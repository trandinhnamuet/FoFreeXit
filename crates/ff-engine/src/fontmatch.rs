//! Khớp & kiểm tra font cho chỉnh sửa text (Phase 4 iteration 2).
//!
//! Mục tiêu chuẩn Foxit: sửa text KHÔNG được đổi font. Chiến lược 3 tầng
//! (dùng ở `edit.rs`):
//! 1. Giữ nguyên font gốc của object nếu font mã hoá được text mới
//!    (`FPDFText_SetText` dùng charmap của font hiện tại) — kiểm bằng
//!    charset-subset hoặc cmap của font bytes (`coverage_ok`).
//! 2. Nếu font gốc thiếu glyph (subset) → tìm font HỆ THỐNG CÙNG HỌ
//!    (`find_family_font_bytes`) đúng đậm/nghiêng — trông gần như y hệt.
//! 3. Bất đắc dĩ mới rơi về font mặc định (`annot::find_font_bytes`).

/// Chuẩn hoá tên family để so khớp: chữ thường, chỉ giữ chữ+số.
pub(crate) fn normalize_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Tách tên font PDF (PostScript name) thành (family sạch, đậm?, nghiêng?).
/// Ví dụ: "ABCDEF+TimesNewRomanPS-BoldItalicMT" → ("TimesNewRoman", true, true).
pub(crate) fn clean_font_name(raw: &str) -> (String, bool, bool) {
    let mut name = raw.trim();
    // Bỏ prefix subset "ABCDEF+".
    let bytes = name.as_bytes();
    if bytes.len() > 7 && bytes[6] == b'+' && bytes[..6].iter().all(|b| b.is_ascii_uppercase()) {
        name = &name[7..];
    }
    let lower = name.to_ascii_lowercase();
    let bold = ["bold", "black", "heavy", "semibold", "demibold"]
        .iter()
        .any(|k| lower.contains(k));
    let italic = lower.contains("italic") || lower.contains("oblique");

    // Family = phần trước dấu '-' hoặc ',' đầu tiên (quy ước PostScript).
    let mut family = name.split(['-', ',']).next().unwrap_or(name).to_string();
    // Bỏ từ khoá kiểu dính liền cuối family ("ArialBold", "CalibriItalic"...).
    let lower_fam = family.to_ascii_lowercase();
    for suffix in ["bolditalic", "boldoblique", "bold", "italic", "oblique", "regular"] {
        if lower_fam.ends_with(suffix) && lower_fam.len() > suffix.len() {
            family.truncate(family.len() - suffix.len());
            break;
        }
    }
    // Bỏ hậu tố PostScript vô nghĩa với người dùng.
    for suffix in ["PSMT", "PS", "MT"] {
        if family.ends_with(suffix) && family.len() > suffix.len() {
            family.truncate(family.len() - suffix.len());
        }
    }
    (family.trim().to_string(), bold, italic)
}

/// Font bytes (TTF/OTF/TTC) có glyph cho MỌI ký tự của `text` không?
/// Parse hỏng (Type1 thuần...) → false (caller sẽ thử đường khác).
pub(crate) fn coverage_ok(font_bytes: &[u8], text: &str) -> bool {
    let n = ttf_parser::fonts_in_collection(font_bytes).unwrap_or(1).max(1);
    'face: for i in 0..n {
        let face = match ttf_parser::Face::parse(font_bytes, i) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            if face.glyph_index(ch).is_none() {
                continue 'face;
            }
        }
        return true;
    }
    false
}

/// Tìm font HỆ THỐNG cùng family (đúng đậm/nghiêng) — trả về bytes TTF.
/// Family so khớp mềm theo `normalize_key` (chứa nhau, tối thiểu 4 ký tự).
pub(crate) fn find_family_font_bytes(family: &str, bold: bool, italic: bool) -> Option<Vec<u8>> {
    let mut key = normalize_key(family);
    if key.is_empty() {
        return None;
    }
    // Alias các tên chuẩn PDF (base-14) → family hệ thống tương đương.
    if key == "helvetica" || key == "helveticaneue" {
        key = "arial".into();
    } else if key == "times" || key == "timesroman" {
        key = "timesnewroman".into();
    } else if key == "courier" {
        key = "couriernew".into();
    }
    let style_idx = match (bold, italic) {
        (false, false) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (true, true) => 3,
    };

    #[cfg(windows)]
    {
        // (key, [regular, bold, italic, bold-italic]) trong C:\Windows\Fonts.
        const TABLE: &[(&str, [&str; 4])] = &[
            ("timesnewroman", ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"]),
            ("arial", ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"]),
            ("calibri", ["calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"]),
            ("cambria", ["cambria.ttc", "cambriab.ttf", "cambriai.ttf", "cambriaz.ttf"]),
            ("candara", ["candara.ttf", "candarab.ttf", "candarai.ttf", "candaraz.ttf"]),
            ("georgia", ["georgia.ttf", "georgiab.ttf", "georgiai.ttf", "georgiaz.ttf"]),
            ("verdana", ["verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"]),
            ("tahoma", ["tahoma.ttf", "tahomabd.ttf", "tahoma.ttf", "tahomabd.ttf"]),
            ("trebuchetms", ["trebuc.ttf", "trebucbd.ttf", "trebucit.ttf", "trebucbi.ttf"]),
            ("couriernew", ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"]),
            ("segoeui", ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"]),
            ("consolas", ["consola.ttf", "consolab.ttf", "consolai.ttf", "consolaz.ttf"]),
            ("comicsansms", ["comic.ttf", "comicbd.ttf", "comici.ttf", "comicz.ttf"]),
            ("impact", ["impact.ttf", "impact.ttf", "impact.ttf", "impact.ttf"]),
            ("garamond", ["gara.ttf", "garabd.ttf", "garait.ttf", "garabd.ttf"]),
            ("palatinolinotype", ["pala.ttf", "palab.ttf", "palai.ttf", "palabi.ttf"]),
        ];
        for (k, files) in TABLE {
            if keys_match(&key, k) {
                if let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{}", files[style_idx])) {
                    return Some(bytes);
                }
            }
        }
        None
    }

    #[cfg(target_os = "macos")]
    {
        const TABLE: &[(&str, &str)] = &[
            ("timesnewroman", "Times New Roman"),
            ("arial", "Arial"),
            ("georgia", "Georgia"),
            ("verdana", "Verdana"),
            ("tahoma", "Tahoma"),
            ("trebuchetms", "Trebuchet MS"),
            ("couriernew", "Courier New"),
            ("comicsansms", "Comic Sans MS"),
            ("impact", "Impact"),
        ];
        let style = ["", " Bold", " Italic", " Bold Italic"][style_idx];
        for (k, display) in TABLE {
            if keys_match(&key, k) {
                for dir in ["/System/Library/Fonts/Supplemental", "/Library/Fonts"] {
                    if let Ok(bytes) = std::fs::read(format!("{dir}/{display}{style}.ttf")) {
                        return Some(bytes);
                    }
                }
            }
        }
        None
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // 1) Liberation: tương thích metric với Arial/Times/Courier — ưu tiên.
        let lib = match key.as_str() {
            "arial" => Some("LiberationSans"),
            "timesnewroman" => Some("LiberationSerif"),
            "couriernew" => Some("LiberationMono"),
            _ => None,
        };
        if let Some(base) = lib {
            let style = ["Regular", "Bold", "Italic", "BoldItalic"][style_idx];
            for dir in [
                "/usr/share/fonts/truetype/liberation",
                "/usr/share/fonts/liberation",
                "/usr/share/fonts/TTF",
            ] {
                if let Ok(bytes) = std::fs::read(format!("{dir}/{base}-{style}.ttf")) {
                    return Some(bytes);
                }
            }
        }
        // 2) fontconfig: fc-match trả family+file; chỉ nhận khi family khớp thật
        //    (fc-match luôn trả "gần nhất" kể cả khi không có font đó).
        let style = ["Regular", "Bold", "Italic", "Bold Italic"][style_idx];
        let pattern = format!("{family}:style={style}");
        if let Ok(out) = std::process::Command::new("fc-match")
            .args(["-f", "%{family}\t%{file}", &pattern])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some((fam, file)) = s.trim().split_once('\t') {
                    if keys_match(&key, &normalize_key(fam)) {
                        if let Ok(bytes) = std::fs::read(file) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }
        None
    }
}

/// So khớp mềm 2 key đã chuẩn hoá: bằng nhau, hoặc chứa nhau (đủ dài để tránh nhiễu).
fn keys_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short.len() >= 4 && long.contains(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_subset_postscript_name() {
        let (fam, bold, italic) = clean_font_name("ABCDEF+TimesNewRomanPS-BoldItalicMT");
        assert_eq!(fam, "TimesNewRoman");
        assert!(bold);
        assert!(italic);
    }

    #[test]
    fn clean_simple_names() {
        assert_eq!(clean_font_name("Helvetica"), ("Helvetica".into(), false, false));
        assert_eq!(clean_font_name("Arial-Bold"), ("Arial".into(), true, false));
        assert_eq!(clean_font_name("CalibriItalic"), ("Calibri".into(), false, true));
    }

    #[test]
    fn keys_match_soft() {
        assert!(keys_match("timesnewroman", "timesnewromanps"));
        assert!(!keys_match("arial", "georgia"));
        assert!(keys_match("arial", "arial"));
    }
}
