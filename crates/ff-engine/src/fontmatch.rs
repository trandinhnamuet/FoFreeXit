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

/// Bẻ dòng kiểu Word cho reflow đoạn (Phase 4 iteration 3): greedy theo TỪ,
/// tôn trọng `\n` là ngắt dòng cứng; từ đơn dài hơn bề rộng thì cắt theo ký
/// tự. `measure(c)` trả bề rộng 1 ký tự theo pt (đã nhân cỡ chữ). Dòng rỗng
/// (từ `\n\n`) được GIỮ trong kết quả để caller giữ nhịp baseline.
pub(crate) fn wrap_lines(text: &str, max_width: f32, measure: &dyn Fn(char) -> f32) -> Vec<String> {
    let width_of = |s: &str| s.chars().map(measure).sum::<f32>();
    // Nới 2% để tránh bẻ sớm vì sai số đo (không kerning).
    let limit = max_width.max(1.0) * 1.02;
    let mut out = Vec::new();
    for para in text.split('\n') {
        let words: Vec<&str> = para.split_whitespace().collect();
        if words.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in words {
            let candidate_extra = if line.is_empty() { width_of(word) } else { measure(' ') + width_of(word) };
            if !line.is_empty() && width_of(&line) + candidate_extra > limit {
                out.push(std::mem::take(&mut line));
            }
            if line.is_empty() && width_of(word) > limit {
                // Từ đơn quá dài: cắt theo ký tự.
                let mut piece = String::new();
                for ch in word.chars() {
                    if !piece.is_empty() && width_of(&piece) + measure(ch) > limit {
                        out.push(std::mem::take(&mut piece));
                    }
                    piece.push(ch);
                }
                line = piece;
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

/// Bề rộng 1 ký tự (pt) theo hmtx của font tại cỡ `size_pt`. Glyph thiếu hoặc
/// không đo được → xấp xỉ 0.5em (đủ tốt cho bẻ dòng, không dùng để vẽ).
pub(crate) fn char_advance(face: &ttf_parser::Face<'_>, ch: char, size_pt: f32) -> f32 {
    let upem = face.units_per_em() as f32;
    face.glyph_index(ch)
        .and_then(|g| face.glyph_hor_advance(g))
        .map(|adv| adv as f32 / upem * size_pt)
        .unwrap_or(size_pt * 0.5)
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

    #[test]
    fn wrap_greedy_by_words() {
        // Mỗi ký tự rộng 10pt (kể cả space) → bề rộng 35pt chứa tối đa 3 ký tự/dòng…
        // dùng câu chữ 2-2-3 để kiểm greedy: "ab cd efg", limit 5 ký tự (50pt).
        let m = |_c: char| 10.0;
        let lines = wrap_lines("ab cd efg", 50.0, &m);
        assert_eq!(lines, vec!["ab cd".to_string(), "efg".to_string()]);
    }

    #[test]
    fn wrap_respects_hard_breaks_and_empty_lines() {
        let m = |_c: char| 10.0;
        let lines = wrap_lines("mot\n\nhai ba", 100.0, &m);
        assert_eq!(lines, vec!["mot".to_string(), String::new(), "hai ba".to_string()]);
    }

    #[test]
    fn wrap_splits_overlong_word() {
        let m = |_c: char| 10.0;
        let lines = wrap_lines("abcdefgh", 30.0, &m);
        // limit 30*1.02 → 3 ký tự/dòng
        assert_eq!(lines, vec!["abc".to_string(), "def".to_string(), "gh".to_string()]);
    }
}
