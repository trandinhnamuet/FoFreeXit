//! Trao đổi dữ liệu form: XFDF (XML chuẩn Adobe), XML đơn giản, TXT (tab —
//! như Acrobat "Export to tab-delimited"), CSV; cùng FDF ở `form.rs`.
//! `export_form_data` / `import_form_data` chọn định dạng theo phần mở rộng.

use std::path::Path;

use lopdf::Document;

use crate::form::FieldKind;
use crate::formx::{self, FillValue};
use crate::EngineError;

fn le<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> EngineError + '_ {
    move |e| EngineError::Pdfium(format!("form {ctx}: {e}"))
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\r' => o.push_str("&#13;"),
            c => o.push(c),
        }
    }
    o
}

// ---------- XFDF ----------

/// Cây tên field (a.b.c) để xuất XFDF lồng nhau đúng chuẩn.
#[derive(Default)]
struct Node {
    children: Vec<(String, Node)>,
    values: Option<Vec<String>>,
}

impl Node {
    fn insert(&mut self, parts: &[&str], values: Vec<String>) {
        let Some((first, rest)) = parts.split_first() else {
            self.values = Some(values);
            return;
        };
        let idx = match self.children.iter().position(|(n, _)| n == first) {
            Some(i) => i,
            None => {
                self.children.push((first.to_string(), Node::default()));
                self.children.len() - 1
            }
        };
        self.children[idx].1.insert(rest, values);
    }
    fn write(&self, out: &mut String, depth: usize) {
        for (name, n) in &self.children {
            let ind = "  ".repeat(depth);
            out.push_str(&format!("{ind}<field name=\"{}\">\n", xml_escape(name)));
            if let Some(vals) = &n.values {
                for v in vals {
                    out.push_str(&format!("{ind}  <value>{}</value>\n", xml_escape(v)));
                }
            }
            n.write(out, depth + 1);
            out.push_str(&format!("{ind}</field>\n"));
        }
    }
}

fn collect(input: &Path) -> Result<Vec<(String, FieldKind, Vec<String>)>, EngineError> {
    let doc = Document::load(input).map_err(le("load"))?;
    Ok(formx::field_values(&doc))
}

/// Xuất XFDF (UTF-8, lồng theo tên phân cấp, list đa chọn = nhiều <value>).
pub fn export_xfdf(input: &Path, output: &Path) -> Result<(), EngineError> {
    let vals = collect(input)?;
    let mut root = Node::default();
    for (name, _, v) in vals {
        let parts: Vec<&str> = name.split('.').collect();
        root.insert(&parts, v);
    }
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n<fields>\n");
    root.write(&mut s, 1);
    s.push_str("</fields>\n");
    if let Some(name) = input.file_name() {
        s.push_str(&format!("<f href=\"{}\"/>\n", xml_escape(&name.to_string_lossy())));
    }
    s.push_str("</xfdf>\n");
    std::fs::write(output, s)?;
    Ok(())
}

/// XML đơn giản: `<fields><field name=".." type="text">giá trị</field>...`.
pub fn export_xml(input: &Path, output: &Path) -> Result<(), EngineError> {
    let vals = collect(input)?;
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<fields>\n");
    for (name, kind, v) in vals {
        if v.len() > 1 {
            s.push_str(&format!("  <field name=\"{}\" type=\"{}\">\n", xml_escape(&name), kind.as_str()));
            for x in v {
                s.push_str(&format!("    <value>{}</value>\n", xml_escape(&x)));
            }
            s.push_str("  </field>\n");
        } else {
            s.push_str(&format!(
                "  <field name=\"{}\" type=\"{}\">{}</field>\n",
                xml_escape(&name),
                kind.as_str(),
                xml_escape(v.first().map(|s| s.as_str()).unwrap_or(""))
            ));
        }
    }
    s.push_str("</fields>\n");
    std::fs::write(output, s)?;
    Ok(())
}

/// TXT phân cách tab (dòng 1 = tên, dòng 2 = giá trị) — định dạng Acrobat.
pub fn export_txt(input: &Path, output: &Path) -> Result<(), EngineError> {
    let vals = collect(input)?;
    let clean = |s: &str| s.replace(['\t', '\r', '\n'], " ");
    let names: Vec<String> = vals.iter().map(|v| clean(&v.0)).collect();
    let values: Vec<String> = vals.iter().map(|v| clean(&v.2.join(";"))).collect();
    std::fs::write(output, format!("{}\n{}\n", names.join("\t"), values.join("\t")))?;
    Ok(())
}

// ---------- XML parser tối giản (đủ cho XFDF / XML ta xuất) ----------

#[derive(Debug)]
enum Tok {
    Open(String, Vec<(String, String)>, bool),
    Close(String),
    Text(String),
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let tail = &rest[p..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            rest = "";
            break;
        };
        let ent = &tail[1..end];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn tokenize(xml: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut rest = xml;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("<!--") {
            rest = stripped.find("-->").map(|e| &stripped[e + 3..]).unwrap_or("");
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("<![CDATA[") {
            let e = stripped.find("]]>").unwrap_or(stripped.len());
            toks.push(Tok::Text(stripped[..e].to_string()));
            rest = stripped.get(e + 3..).unwrap_or("");
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = rest.find('>').map(|e| &rest[e + 1..]).unwrap_or("");
            continue;
        }
        if let Some(stripped) = rest.strip_prefix('<') {
            let e = stripped.find('>').unwrap_or(stripped.len());
            let inner = &stripped[..e];
            rest = stripped.get(e + 1..).unwrap_or("");
            if let Some(name) = inner.strip_prefix('/') {
                toks.push(Tok::Close(local(name.trim())));
                continue;
            }
            let self_close = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let mut chars = inner.char_indices();
            let name_end = chars.find(|(_, c)| c.is_whitespace()).map(|(i, _)| i).unwrap_or(inner.len());
            let name = local(&inner[..name_end]);
            let mut attrs = Vec::new();
            let mut a = &inner[name_end..];
            loop {
                a = a.trim_start();
                let Some(eq) = a.find('=') else { break };
                let key = local(a[..eq].trim());
                let after = a[eq + 1..].trim_start();
                let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
                let Some(end) = after[1..].find(q) else { break };
                attrs.push((key, unescape(&after[1..1 + end])));
                a = &after[end + 2..];
            }
            toks.push(Tok::Open(name, attrs, self_close));
            continue;
        }
        let e = rest.find('<').unwrap_or(rest.len());
        toks.push(Tok::Text(unescape(&rest[..e])));
        rest = &rest[e..];
    }
    toks
}

fn local(n: &str) -> String {
    n.rsplit(':').next().unwrap_or(n).to_string()
}

/// Đọc XFDF (lồng hoặc tên chấm) hoặc XML ta xuất → danh sách giá trị.
pub fn parse_xfdf(path: &Path) -> Result<Vec<FillValue>, EngineError> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes)).into_owned();
    let mut out: Vec<FillValue> = Vec::new();
    let mut stack: Vec<(String, Vec<String>, String, bool)> = Vec::new(); // (tên đầy đủ, values, text trực tiếp, có <value>)
    let mut in_value = false;
    let mut value_buf = String::new();
    for tok in tokenize(&text) {
        match tok {
            Tok::Open(name, attrs, self_close) if name == "field" => {
                let n = attrs.iter().find(|(k, _)| k == "name").map(|(_, v)| v.clone()).unwrap_or_default();
                let full = match stack.last() {
                    Some((p, ..)) if !p.is_empty() => format!("{p}.{n}"),
                    _ => n,
                };
                if self_close {
                    out.push(FillValue { name: full, value: String::new(), values: vec![] });
                } else {
                    stack.push((full, vec![], String::new(), false));
                }
            }
            Tok::Open(name, _, self_close) if name == "value" || name == "value-richtext" => {
                if self_close {
                    if let Some(top) = stack.last_mut() {
                        top.1.push(String::new());
                        top.3 = true;
                    }
                } else {
                    in_value = true;
                    value_buf.clear();
                }
            }
            Tok::Close(name) if name == "value" || name == "value-richtext" => {
                if in_value {
                    if let Some(top) = stack.last_mut() {
                        top.1.push(std::mem::take(&mut value_buf));
                        top.3 = true;
                    }
                }
                in_value = false;
            }
            Tok::Close(name) if name == "field" => {
                if let Some((full, vals, direct, has_value)) = stack.pop() {
                    if has_value {
                        let first = vals.first().cloned().unwrap_or_default();
                        out.push(FillValue { name: full, value: first, values: if vals.len() > 1 { vals } else { vec![] } });
                    } else if !direct.trim().is_empty() {
                        out.push(FillValue { name: full, value: direct.trim().to_string(), values: vec![] });
                    }
                }
            }
            Tok::Text(t) => {
                if in_value {
                    value_buf.push_str(&t);
                } else if let Some(top) = stack.last_mut() {
                    top.2.push_str(&t);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, q) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => q = !q,
            (',', false) => cells.push(std::mem::take(&mut cur)),
            (c, _) => cur.push(c),
        }
    }
    cells.push(cur);
    cells
}

/// CSV 2 cột name,value (định dạng `export_csv`).
pub fn parse_csv(path: &Path) -> Result<Vec<FillValue>, EngineError> {
    let text = std::fs::read_to_string(path)?;
    let text = text.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    // Ghép dòng khi ô có xuống dòng trong ngoặc kép.
    let mut buf = String::new();
    for line in text.lines() {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
        if buf.matches('"').count() % 2 == 1 {
            continue;
        }
        let cells = parse_csv_line(&buf);
        buf.clear();
        if cells.len() >= 2 && !(cells[0] == "name" && cells[1] == "value") {
            out.push(FillValue { name: cells[0].clone(), value: cells[1].clone(), values: vec![] });
        }
    }
    Ok(out)
}

/// TXT tab (dòng tên + dòng giá trị).
pub fn parse_txt(path: &Path) -> Result<Vec<FillValue>, EngineError> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let names: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let values: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    Ok(names
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.trim().is_empty())
        .map(|(i, n)| FillValue { name: n.to_string(), value: values.get(i).copied().unwrap_or("").to_string(), values: vec![] })
        .collect())
}

/// Xuất dữ liệu form theo đuôi: .fdf, .xfdf, .xml, .txt, .csv.
pub fn export_form_data(input: &Path, output: &Path) -> Result<(), EngineError> {
    match ext_of(output).as_str() {
        "xfdf" => export_xfdf(input, output),
        "xml" => export_xml(input, output),
        "txt" => export_txt(input, output),
        "csv" => crate::form::export_csv(input, output),
        _ => crate::form::export_fdf(input, output),
    }
}

/// Đọc file dữ liệu (theo đuôi) → giá trị điền.
pub fn read_form_data(data: &Path) -> Result<Vec<FillValue>, EngineError> {
    match ext_of(data).as_str() {
        "xfdf" | "xml" => parse_xfdf(data),
        "csv" => parse_csv(data),
        "txt" => parse_txt(data),
        _ => Ok(crate::form::parse_fdf(data)?
            .into_iter()
            .map(|v| FillValue { name: v.name, value: v.value, values: vec![] })
            .collect()),
    }
}

/// Nhập dữ liệu form (FDF/XFDF/XML/CSV/TXT) vào PDF, ghi `output`. Trả số field đã điền.
pub fn import_form_data(input: &Path, data: &Path, output: &Path) -> Result<usize, EngineError> {
    let vals = read_form_data(data)?;
    formx::fill_form(input, &vals, output)
}
