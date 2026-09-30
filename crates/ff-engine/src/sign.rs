//! Chữ ký số PDF — PAdES/PKCS#7 detached (adbe.pkcs7.detached), RSA.
//!
//! 1. Digital ID: `generate_self_signed_id` (PEM) / `generate_self_signed_pfx`
//!    (PFX/P12 có mật khẩu — như "Tạo Digital ID mới" của Foxit), nạp ID từ
//!    PEM hoặc PFX/P12 (`load_identity_file`, parser PKCS#12 thuần Rust hỗ trợ
//!    cả PBES2/AES lẫn 3DES/RC2 cũ), `identity_info` để UI hiện "Ký với tư cách".
//! 2. `sign_pdf_ex` — ký kiểu Foxit/Adobe bằng CẬP NHẬT TĂNG DẦN (incremental
//!    update): giữ nguyên từng byte của bản gốc (chữ ký trước đó vẫn hợp lệ),
//!    nối thêm: signature dict (/ByteRange + /Contents placeholder độ rộng cố
//!    định), widget /Sig (ẩn, hoặc HIỂN THỊ với /AP dựng từ tên/ngày/lý do/
//!    nơi ký + ảnh chữ ký), AcroForm /SigFlags 3, xref (bảng hoặc xref stream
//!    tuỳ bản gốc) + trailer /Prev. Có thể ký vào ô chữ ký TRỐNG sẵn có.
//!    Sau khi ghi xong mới vá ByteRange thật, băm SHA-256 phần ngoài Contents
//!    → CMS SignedData detached (signed attrs: contentType, messageDigest,
//!    signingTime; kèm cả chuỗi chứng chỉ trong PFX) → hex vào Contents.
//! 3. `verify_signatures` — mỗi chữ ký: băm lại ByteRange, kiểm chữ ký RSA trên
//!    signed attrs + messageDigest, chữ ký có phủ toàn bộ file không (sửa sau
//!    khi ký), các thay đổi sau đó có phải chỉ là chữ ký khác không, thông tin
//!    người ký/thời gian/lý do/nơi ký và chi tiết chứng chỉ (subject, issuer,
//!    hiệu lực, tự ký).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use der::{Decode, Encode};
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};
use rsa::pkcs1v15::{SigningKey, VerifyingKey};
use rsa::signature::Verifier;
use rsa::RsaPrivateKey;
use sha2::{Digest, Sha256};

use crate::qpdf;
use crate::EngineError;

/// Kích thước (byte) dành cho Contents chữ ký. CMS RSA-4096 + chuỗi 3-4 cert
/// ~8KB; 24KB dư cho cả timestamp sau này.
const CONTENTS_CAPACITY: usize = 24576;

/// Placeholder ByteRange độ rộng cố định (vá tại chỗ sau khi biết offset).
const BYTERANGE_PLACEHOLDER: &str = "[0 0000000000 0000000000 0000000000]";

fn eng<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> EngineError + '_ {
    move |e| EngineError::Pdfium(format!("{ctx}: {e}"))
}

fn fail(msg: impl Into<String>) -> EngineError {
    EngineError::Pdfium(msg.into())
}

// =====================================================================
// Digital ID
// =====================================================================

/// Tạo khoá RSA-2048 + chứng chỉ tự ký. Trả (cert DER, key PKCS#8 PEM, rcgen keypair).
fn make_self_signed(
    common_name: &str,
    organization: &str,
    email: &str,
) -> Result<(Vec<u8>, String, String), EngineError> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_RSA_SHA256};

    let mut rng = rsa::rand_core::OsRng;
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).map_err(eng("tạo khoá RSA"))?;
    let key_pem = {
        use rsa::pkcs8::EncodePrivateKey;
        priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(eng("mã hoá PKCS#8"))?
            .to_string()
    };
    let key_pair = KeyPair::from_pkcs8_pem_and_sign_algo(&key_pem, &PKCS_RSA_SHA256)
        .map_err(eng("rcgen keypair"))?;

    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(eng("params"))?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    let org = if organization.trim().is_empty() { "FoFreeXit Self-Signed" } else { organization.trim() };
    dn.push(DnType::OrganizationName, org);
    params.distinguished_name = dn;
    if !email.trim().is_empty() {
        if let Ok(ia5) = rcgen::Ia5String::try_from(email.trim().to_string()) {
            params.subject_alt_names.push(rcgen::SanType::Rfc822Name(ia5));
        }
    }
    // Hiệu lực 5 năm từ hôm nay (Foxit mặc định 5 năm cho ID tự ký).
    let now = time_now_secs() as i64;
    params.not_before = rcgen::date_time_ymd(civil(now).0 as i32, civil(now).1 as u8, civil(now).2 as u8);
    let later = civil(now + 5 * 365 * 86400);
    params.not_after = rcgen::date_time_ymd(later.0 as i32, later.1 as u8, later.2 as u8);
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::DigitalSignature,
        rcgen::KeyUsagePurpose::ContentCommitment,
    ];
    let cert = params.self_signed(&key_pair).map_err(eng("tự ký cert"))?;
    Ok((cert.der().to_vec(), cert.pem(), key_pair.serialize_pem()))
}

/// Tạo Digital ID tự ký, ghi ra `out_pem` (cert PEM + private key PEM nối nhau).
/// `common_name` là tên hiển thị trong chữ ký (vd "Nguyen Van A").
pub fn generate_self_signed_id(common_name: &str, out_pem: &Path) -> Result<(), EngineError> {
    let (_der, cert_pem, key_pem) = make_self_signed(common_name, "", "")?;
    let mut bundle = cert_pem;
    bundle.push('\n');
    bundle.push_str(&key_pem);
    std::fs::write(out_pem, bundle)?;
    Ok(())
}

/// Tạo Digital ID tự ký dạng PFX/P12 (PKCS#12) bảo vệ bằng `password` —
/// định dạng Foxit/Adobe/Windows dùng. Mở lại được bằng `load_identity_file`.
pub fn generate_self_signed_pfx(
    common_name: &str,
    organization: &str,
    email: &str,
    password: &str,
    out_pfx: &Path,
) -> Result<(), EngineError> {
    if password.is_empty() {
        return Err(fail("Digital ID PFX cần mật khẩu"));
    }
    let (cert_der, _cert_pem, key_pem) = make_self_signed(common_name, organization, email)?;
    let key_der = pem_iter(&key_pem)
        .into_iter()
        .find(|b| b.tag == "PRIVATE KEY")
        .map(|b| b.der)
        .ok_or_else(|| fail("thiếu private key"))?;
    let bytes = build_pfx(&cert_der, &key_der, common_name, password)?;
    std::fs::write(out_pfx, bytes)?;
    Ok(())
}

/// Đóng gói (cert DER, key PKCS#8 DER) thành PFX có mật khẩu (PBES2/AES-256 + HMAC-SHA256).
pub fn build_pfx(cert_der: &[u8], key_pkcs8_der: &[u8], alias: &str, password: &str) -> Result<Vec<u8>, EngineError> {
    use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKeyChain};
    let cert = Certificate::from_der(cert_der).map_err(eng("PFX: đọc cert"))?;
    let local_key_id: Vec<u8> = Sha256::digest(cert_der)[..20].to_vec();
    let chain = PrivateKeyChain::new(key_pkcs8_der, local_key_id, vec![cert]);
    let mut ks = KeyStore::new();
    let alias = if alias.trim().is_empty() { "digital-id" } else { alias.trim() };
    ks.add_entry(alias, KeyStoreEntry::PrivateKeyChain(chain));
    ks.writer(password).write().map_err(eng("PFX: ghi"))
}

/// Digital ID đã nạp: chuỗi cert (cert người ký ĐẦU TIÊN) + khoá RSA.
pub struct Identity {
    pub cert_chain: Vec<Vec<u8>>,
    key: RsaPrivateKey,
}

/// Thông tin tóm tắt Digital ID cho UI.
#[derive(Clone, Debug, Default)]
pub struct IdentityInfo {
    pub common_name: String,
    pub subject: String,
    pub issuer: String,
    pub email: String,
    pub not_before: String,
    pub not_after: String,
    pub self_signed: bool,
    /// Đã hết hạn / chưa tới hạn tại thời điểm hiện tại.
    pub expired: bool,
}

/// Nạp Digital ID từ file PEM (cert + key) hoặc PFX/P12 (cần `password`).
pub fn load_identity_file(path: &Path, password: Option<&str>) -> Result<Identity, EngineError> {
    let bytes = std::fs::read(path)?;
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]).to_string();
    if head.trim_start_matches('\u{feff}').trim_start().starts_with("-----BEGIN") {
        let (cert, key) = load_identity_pem(&String::from_utf8_lossy(&bytes))?;
        return Ok(Identity { cert_chain: vec![cert], key });
    }
    load_identity_pfx(&bytes, password.unwrap_or(""))
}

fn load_identity_pfx(bytes: &[u8], password: &str) -> Result<Identity, EngineError> {
    use rsa::pkcs8::DecodePrivateKey;
    let ks = p12_keystore::KeyStore::from_pkcs12(bytes, password).map_err(|e| {
        let s = e.to_string();
        if s.to_lowercase().contains("mac") || s.to_lowercase().contains("password") || s.to_lowercase().contains("decrypt") {
            fail(format!("sai mật khẩu Digital ID (PFX/P12): {s}"))
        } else {
            fail(format!("không đọc được PFX/P12: {s}"))
        }
    })?;
    let (_alias, chain) = ks
        .private_key_chain()
        .ok_or_else(|| fail("PFX/P12 không chứa khoá riêng (private key)"))?;
    let key = RsaPrivateKey::from_pkcs8_der(chain.key())
        .map_err(|e| fail(format!("khoá riêng trong PFX không phải RSA (hiện hỗ trợ RSA): {e}")))?;
    let mut certs: Vec<Vec<u8>> = chain.chain().iter().map(|c| c.as_der().to_vec()).collect();
    if certs.is_empty() {
        return Err(fail("PFX/P12 không chứa chứng chỉ"));
    }
    // Đưa cert KHỚP khoá riêng lên đầu (một số PFX xếp CA trước).
    let pub_der = {
        use rsa::pkcs8::EncodePublicKey;
        key.to_public_key().to_public_key_der().map(|d| d.as_bytes().to_vec()).unwrap_or_default()
    };
    if let Some(pos) = certs.iter().position(|c| {
        x509_cert::Certificate::from_der(c)
            .ok()
            .and_then(|x| x.tbs_certificate.subject_public_key_info.to_der().ok())
            .map(|d| d == pub_der)
            .unwrap_or(false)
    }) {
        let c = certs.remove(pos);
        certs.insert(0, c);
    }
    Ok(Identity { cert_chain: certs, key })
}

/// Tóm tắt Digital ID (kiểm luôn mật khẩu PFX).
pub fn identity_info(path: &Path, password: Option<&str>) -> Result<IdentityInfo, EngineError> {
    let id = load_identity_file(path, password)?;
    let cert = x509_cert::Certificate::from_der(&id.cert_chain[0]).map_err(eng("đọc cert"))?;
    let d = cert_details(&cert);
    let now = time_now_secs();
    Ok(IdentityInfo {
        common_name: common_name(&cert).unwrap_or_default(),
        subject: d.subject,
        issuer: d.issuer,
        email: cert_email(&cert).unwrap_or_default(),
        not_before: d.not_before,
        not_after: d.not_after,
        self_signed: d.self_signed,
        expired: now < d.not_before_secs || now > d.not_after_secs,
    })
}

/// Nạp (cert DER, RSA private key) từ PEM bundle (cert + key nối nhau).
/// Tách từng khối PEM rồi decode DER đúng loại.
fn load_identity_pem(text: &str) -> Result<(Vec<u8>, RsaPrivateKey), EngineError> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    let blocks = pem_iter(text);
    let cert_der = blocks
        .iter()
        .find(|b| b.tag == "CERTIFICATE")
        .map(|b| b.der.clone())
        .ok_or_else(|| fail("PEM thiếu CERTIFICATE"))?;
    let key_block = blocks
        .iter()
        .find(|b| b.tag == "PRIVATE KEY" || b.tag == "RSA PRIVATE KEY")
        .ok_or_else(|| fail("PEM thiếu PRIVATE KEY"))?;
    let priv_key = if key_block.tag == "RSA PRIVATE KEY" {
        RsaPrivateKey::from_pkcs1_der(&key_block.der).map_err(eng("đọc RSA key (PKCS#1)"))?
    } else {
        RsaPrivateKey::from_pkcs8_der(&key_block.der).map_err(eng("đọc RSA key (PKCS#8)"))?
    };
    Ok((cert_der, priv_key))
}

struct PemBlock {
    tag: String,
    der: Vec<u8>,
}

/// Parser PEM tối giản: tách các khối `-----BEGIN X-----` … `-----END X-----`.
fn pem_iter(text: &str) -> Vec<PemBlock> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("-----BEGIN ") {
            let tag = rest.trim_end_matches('-').trim().to_string();
            let mut b64 = String::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("-----END") {
                    break;
                }
                b64.push_str(l.trim());
            }
            if let Some(der) = base64_decode(&b64) {
                out.push(PemBlock { tag, der });
            }
        }
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const INV: u8 = 0xFF;
    let mut table = [INV; 256];
    for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[*c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut nbits = 0u32;
    for &b in s.as_bytes() {
        if b == b'=' || b.is_ascii_whitespace() {
            continue;
        }
        let v = table[b as usize];
        if v == INV {
            return None;
        }
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    Some(out)
}

// =====================================================================
// Ký
// =====================================================================

/// Giao diện hiển thị của chữ ký (chữ ký "visible").
#[derive(Clone, Debug, Default)]
pub struct SigAppearance {
    /// Trang (0-based) — bỏ qua khi ký vào ô có sẵn.
    pub page: u16,
    /// [left, bottom, right, top] điểm PDF — bỏ qua khi ký vào ô có sẵn.
    pub rect: [f32; 4],
    /// Ảnh chữ ký (PNG/JPG, nền trong suốt) đặt bên trái khung.
    pub image_path: Option<String>,
    /// Chữ lớn bên trái khi không có ảnh (thường là tên người ký). Rỗng = không.
    pub big_text: String,
    /// Các dòng chi tiết bên phải (đã dịch + định dạng sẵn ở UI): "Ký bởi…",
    /// "Ngày: …", "Lý do: …", "Nơi ký: …".
    pub lines: Vec<String>,
}

/// Yêu cầu ký.
#[derive(Clone, Debug, Default)]
pub struct SignRequest {
    pub reason: String,
    pub location: String,
    pub contact_info: String,
    /// Tên hiển thị (/Name); rỗng = CN của chứng chỉ.
    pub signer_name: String,
    /// Lệch múi giờ địa phương so với UTC (phút), để /M đúng giờ máy.
    pub tz_offset_min: i32,
    /// Ký vào ô chữ ký trống có sẵn (tên đầy đủ). None = tạo ô mới.
    pub field_name: Option<String>,
    /// None = chữ ký ẩn.
    pub appearance: Option<SigAppearance>,
}

/// Ô chữ ký trong tài liệu.
#[derive(Clone, Debug)]
pub struct SigFieldInfo {
    pub name: String,
    pub page_index: Option<u16>,
    pub rect: Option<[f32; 4]>,
    pub signed: bool,
}

/// Ký `input` bằng identity PEM ở `id_pem` (chữ ký ẩn). Giữ để tương thích.
pub fn sign_pdf(
    input: &Path,
    id_pem: &Path,
    reason: &str,
    signer_name: &str,
    output: &Path,
) -> Result<(), EngineError> {
    let id = load_identity_file(id_pem, None)?;
    let req = SignRequest {
        reason: reason.to_string(),
        signer_name: signer_name.to_string(),
        ..Default::default()
    };
    sign_pdf_ex(None, input, &id, &req, output)
}

/// Liệt kê ô chữ ký (/FT /Sig) của tài liệu.
pub fn list_signature_fields(input: &Path) -> Result<Vec<SigFieldInfo>, EngineError> {
    let doc = Document::load(input).map_err(eng("lopdf load"))?;
    let page_of = widget_pages(&doc);
    let mut out = Vec::new();
    for f in collect_sig_fields(&doc) {
        let (page_index, rect) = match f.widget {
            Some(w) => (page_of.get(&w).copied(), widget_rect(&doc, w)),
            None => (None, None),
        };
        out.push(SigFieldInfo { name: f.name, page_index, rect, signed: f.signed });
    }
    Ok(out)
}

struct SigFieldRef {
    name: String,
    field: ObjectId,
    widget: Option<ObjectId>,
    signed: bool,
}

fn resolve<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

fn dict_of<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    resolve(doc, o).as_dict().ok()
}

fn pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let u: Vec<u16> = bytes[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        // PDFDocEncoding ~ Latin-1 cho phần ASCII/Latin.
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Mọi field chữ ký (duyệt đệ quy /Fields → /Kids, FT thừa kế).
fn collect_sig_fields(doc: &Document) -> Vec<SigFieldRef> {
    let mut out = Vec::new();
    let Ok(cat) = doc.catalog() else { return out };
    let Some(acro) = cat.get(b"AcroForm").ok().and_then(|o| dict_of(doc, o)) else { return out };
    let Some(fields) = acro.get(b"Fields").ok().map(|o| resolve(doc, o)).and_then(|o| o.as_array().ok()) else {
        return out;
    };
    fn walk(doc: &Document, id: ObjectId, prefix: &str, ft: Option<Vec<u8>>, depth: u32, out: &mut Vec<SigFieldRef>) {
        if depth > 32 {
            return;
        }
        let Ok(d) = doc.get_dictionary(id) else { return };
        let part = d.get(b"T").ok().and_then(|o| o.as_str().ok()).map(pdf_text);
        let name = match (&part, prefix.is_empty()) {
            (Some(p), true) => p.clone(),
            (Some(p), false) => format!("{prefix}.{p}"),
            (None, _) => prefix.to_string(),
        };
        let ft = d.get(b"FT").ok().and_then(|o| o.as_name().ok()).map(|n| n.to_vec()).or(ft);
        let kids: Vec<ObjectId> = d
            .get(b"Kids")
            .ok()
            .map(|o| resolve(doc, o))
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().filter_map(|k| k.as_reference().ok()).collect())
            .unwrap_or_default();
        // Kids có /T = field con; Kids không /T = widget của field này.
        let field_kids: Vec<ObjectId> = kids
            .iter()
            .copied()
            .filter(|k| doc.get_dictionary(*k).map(|kd| kd.has(b"T")).unwrap_or(false))
            .collect();
        if !field_kids.is_empty() {
            for k in field_kids {
                walk(doc, k, &name, ft.clone(), depth + 1, out);
            }
            return;
        }
        if ft.as_deref() != Some(b"Sig") {
            return;
        }
        let is_widget = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Widget") || d.has(b"Rect");
        let widget = if is_widget { Some(id) } else { kids.first().copied() };
        let signed = d.get(b"V").map(|v| !matches!(resolve(doc, v), Object::Null)).unwrap_or(false);
        out.push(SigFieldRef { name, field: id, widget, signed });
    }
    for f in fields {
        if let Ok(id) = f.as_reference() {
            walk(doc, id, "", None, 0, &mut out);
        }
    }
    out
}

/// widget id → trang (0-based), từ /Annots của từng trang.
fn widget_pages(doc: &Document) -> HashMap<ObjectId, u16> {
    let mut m = HashMap::new();
    for (i, (_, pid)) in doc.get_pages().into_iter().enumerate() {
        if let Ok(p) = doc.get_dictionary(pid) {
            if let Some(arr) = p.get(b"Annots").ok().map(|o| resolve(doc, o)).and_then(|o| o.as_array().ok()) {
                for a in arr {
                    if let Ok(r) = a.as_reference() {
                        m.insert(r, i as u16);
                    }
                }
            }
        }
    }
    m
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn widget_rect(doc: &Document, w: ObjectId) -> Option<[f32; 4]> {
    let d = doc.get_dictionary(w).ok()?;
    let a = resolve(doc, d.get(b"Rect").ok()?).as_array().ok()?;
    if a.len() != 4 {
        return None;
    }
    let v: Vec<f32> = a.iter().filter_map(num).collect();
    if v.len() != 4 {
        return None;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

/// Text string PDF: ASCII → literal; còn lại → UTF-16BE có BOM (hex).
fn text_obj(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(b, StringFormat::Hexadecimal)
    }
}

/// Thông tin xref cuối của bản gốc (để nối incremental update).
struct BaseInfo {
    startxref: usize,
    xref_stream: bool,
}

fn base_info(bytes: &[u8]) -> Option<BaseInfo> {
    let tail_from = bytes.len().saturating_sub(2048);
    let pos = rfind(&bytes[tail_from..], b"startxref")? + tail_from;
    let mut i = pos + 9;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let st = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let off: usize = std::str::from_utf8(&bytes[st..i]).ok()?.parse().ok()?;
    if off >= bytes.len() {
        return None;
    }
    let at = &bytes[off..(off + 64).min(bytes.len())];
    if at.starts_with(b"xref") {
        return Some(BaseInfo { startxref: off, xref_stream: false });
    }
    // "N G obj" … /Type /XRef
    let win = &bytes[off..(off + 512).min(bytes.len())];
    let s = String::from_utf8_lossy(win);
    let head_ok = s.split_whitespace().nth(2).map(|w| w.starts_with("obj")).unwrap_or(false)
        && s.split_whitespace().next().map(|w| w.chars().all(|c| c.is_ascii_digit())).unwrap_or(false);
    if head_ok && s.contains("/XRef") {
        return Some(BaseInfo { startxref: off, xref_stream: true });
    }
    None
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).rev().find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Ký nâng cao: PEM/PFX, lý do/nơi ký/liên hệ, chữ ký hiển thị, ký vào ô có sẵn.
/// `pdfium` cần khi có `appearance` (dựng chữ Unicode + ảnh cho /AP).
pub fn sign_pdf_ex(
    pdfium: Option<&pdfium_render::prelude::Pdfium>,
    input: &Path,
    id: &Identity,
    req: &SignRequest,
    output: &Path,
) -> Result<(), EngineError> {
    let mut base = std::fs::read(input)?;
    // Bản gốc phải đọc được + xref cuối hợp lệ để nối incremental; không thì
    // chuẩn hoá qua qpdf (file hỏng không thể giữ chữ ký cũ được nữa).
    let mut doc = match (Document::load_mem(&base), base_info(&base)) {
        (Ok(d), Some(_)) => d,
        _ => {
            let norm = std::env::temp_dir().join(format!("ff_sign_norm_{}_{}.pdf", std::process::id(), time_nanos()));
            qpdf::repair(input, &norm)?;
            base = std::fs::read(&norm)?;
            let _ = std::fs::remove_file(&norm);
            Document::load_mem(&base).map_err(eng("lopdf load"))?
        }
    };
    let info = base_info(&base).ok_or_else(|| fail("không xác định được xref của tài liệu"))?;
    if doc.trailer.get(b"Encrypt").is_ok() {
        return Err(fail("tài liệu đang được mã hoá — hãy gỡ mật khẩu trước khi ký"));
    }
    doc.version = doc.version.clone();

    let root_id = doc.trailer.get(b"Root").and_then(|o| o.as_reference()).map_err(eng("trailer /Root"))?;
    let size_prev = doc.trailer.get(b"Size").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0).max(0) as u32;
    let mut next_id = size_prev.max(doc.max_id + 1);
    let mut alloc = || {
        let id = (next_id, 0u16);
        next_id += 1;
        id
    };

    // Đối tượng ghi trong bản cập nhật (mới + sửa).
    let mut upd: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let sig_id = alloc();

    let cert = x509_cert::Certificate::from_der(&id.cert_chain[0]).map_err(eng("đọc cert"))?;
    let display_name = if req.signer_name.trim().is_empty() {
        common_name(&cert).unwrap_or_else(|| "Signer".into())
    } else {
        req.signer_name.trim().to_string()
    };

    let pages = doc.get_pages();
    let page_ids: Vec<ObjectId> = pages.values().copied().collect();
    if page_ids.is_empty() {
        return Err(fail("PDF không có trang"));
    }

    // ---- Ô chữ ký: có sẵn hay tạo mới ----
    let existing = match &req.field_name {
        Some(name) => {
            let f = collect_sig_fields(&doc)
                .into_iter()
                .find(|f| &f.name == name)
                .ok_or_else(|| fail(format!("không thấy ô chữ ký \"{name}\"")))?;
            if f.signed {
                return Err(fail(format!("ô chữ ký \"{name}\" đã được ký")));
            }
            Some(f)
        }
        None => None,
    };

    // Trang + khung của widget.
    let (page_id, rect) = if let Some(f) = &existing {
        let w = f.widget.unwrap_or(f.field);
        let pg = widget_pages(&doc).get(&w).map(|i| page_ids[*i as usize]).or_else(|| {
            doc.get_dictionary(w).ok().and_then(|d| d.get(b"P").ok()).and_then(|p| p.as_reference().ok())
        });
        let pg = pg.unwrap_or(page_ids[0]);
        (pg, widget_rect(&doc, w).unwrap_or([0.0; 4]))
    } else if let Some(ap) = &req.appearance {
        let pi = (ap.page as usize).min(page_ids.len() - 1);
        let r = ap.rect;
        (page_ids[pi], [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])])
    } else {
        (page_ids[0], [0.0; 4])
    };

    // ---- /AP (chữ ký hiển thị) ----
    let mut ap_ref: Option<ObjectId> = None;
    if let Some(ap) = &req.appearance {
        let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
        if w >= 4.0 && h >= 4.0 {
            let pdfium = pdfium.ok_or_else(|| fail("cần PDFium để dựng giao diện chữ ký"))?;
            let xobj = build_appearance(pdfium, w, h, ap, &mut alloc, &mut upd)?;
            ap_ref = Some(xobj);
        }
    }

    // ---- Signature dict: ghi thô sau, chỉ giữ chỗ ----
    upd.insert(sig_id, Object::Null);

    let widget_id: ObjectId;
    if let Some(f) = &existing {
        // Field: /V → sig. Widget (có thể chính là field): /AP.
        let mut fd = doc.get_dictionary(f.field).map_err(eng("field"))?.clone();
        fd.set("V", Object::Reference(sig_id));
        let w = f.widget.unwrap_or(f.field);
        widget_id = w;
        if w == f.field {
            if let Some(x) = ap_ref {
                fd.set("AP", Object::Dictionary(Dictionary::from_iter(vec![("N", Object::Reference(x))])));
            }
            let flags = fd.get(b"F").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
            fd.set("F", Object::Integer((flags | 4) & !2 & !1));
            upd.insert(f.field, Object::Dictionary(fd));
        } else {
            upd.insert(f.field, Object::Dictionary(fd));
            let mut wd = doc.get_dictionary(w).map_err(eng("widget"))?.clone();
            if let Some(x) = ap_ref {
                wd.set("AP", Object::Dictionary(Dictionary::from_iter(vec![("N", Object::Reference(x))])));
            }
            let flags = wd.get(b"F").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
            wd.set("F", Object::Integer((flags | 4) & !2 & !1));
            upd.insert(w, Object::Dictionary(wd));
        }
    } else {
        widget_id = alloc();
        let existing_names: Vec<String> = collect_sig_fields(&doc).into_iter().map(|f| f.name).collect();
        let mut n = existing_names.len() + 1;
        while existing_names.iter().any(|x| x == &format!("Signature{n}")) {
            n += 1;
        }
        let mut widget = Dictionary::new();
        widget.set("Type", Object::Name(b"Annot".to_vec()));
        widget.set("Subtype", Object::Name(b"Widget".to_vec()));
        widget.set("FT", Object::Name(b"Sig".to_vec()));
        widget.set("T", Object::string_literal(format!("Signature{n}")));
        widget.set("V", Object::Reference(sig_id));
        widget.set("F", Object::Integer(132)); // Print + Locked
        widget.set("Rect", Object::Array(rect.iter().map(|v| Object::Real(*v)).collect()));
        widget.set("P", Object::Reference(page_id));
        if let Some(x) = ap_ref {
            widget.set("AP", Object::Dictionary(Dictionary::from_iter(vec![("N", Object::Reference(x))])));
        }
        upd.insert(widget_id, Object::Dictionary(widget));

        // /Annots của trang (mảng trực tiếp hoặc gián tiếp).
        let page = doc.get_dictionary(page_id).map_err(eng("page"))?.clone();
        match page.get(b"Annots") {
            Ok(Object::Reference(aid)) => {
                let mut arr = doc.get_object(*aid).ok().and_then(|o| o.as_array().ok().cloned()).unwrap_or_default();
                arr.push(Object::Reference(widget_id));
                upd.insert(*aid, Object::Array(arr));
            }
            Ok(Object::Array(a)) => {
                let mut a = a.clone();
                a.push(Object::Reference(widget_id));
                let mut p = page.clone();
                p.set("Annots", Object::Array(a));
                upd.insert(page_id, Object::Dictionary(p));
            }
            _ => {
                let mut p = page.clone();
                p.set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
                upd.insert(page_id, Object::Dictionary(p));
            }
        }
    }

    // ---- AcroForm: /Fields + /SigFlags 3 ----
    {
        let cat = doc.get_dictionary(root_id).map_err(eng("catalog"))?.clone();
        let add_field = |acro: &mut Dictionary, doc: &Document, upd: &mut BTreeMap<ObjectId, Object>| {
            acro.set("SigFlags", Object::Integer(3));
            if existing.is_none() {
                match acro.get(b"Fields") {
                    Ok(Object::Reference(fid)) => {
                        let fid = *fid;
                        let mut arr = doc.get_object(fid).ok().and_then(|o| o.as_array().ok().cloned()).unwrap_or_default();
                        arr.push(Object::Reference(widget_id));
                        upd.insert(fid, Object::Array(arr));
                    }
                    Ok(Object::Array(a)) => {
                        let mut a = a.clone();
                        a.push(Object::Reference(widget_id));
                        acro.set("Fields", Object::Array(a));
                    }
                    _ => acro.set("Fields", Object::Array(vec![Object::Reference(widget_id)])),
                }
            }
        };
        match cat.get(b"AcroForm") {
            Ok(Object::Reference(aid)) => {
                let aid = *aid;
                let mut acro = doc.get_dictionary(aid).map_err(eng("AcroForm"))?.clone();
                add_field(&mut acro, &doc, &mut upd);
                upd.insert(aid, Object::Dictionary(acro));
            }
            Ok(Object::Dictionary(d)) => {
                let mut acro = d.clone();
                add_field(&mut acro, &doc, &mut upd);
                let mut c = cat.clone();
                c.set("AcroForm", Object::Dictionary(acro));
                upd.insert(root_id, Object::Dictionary(c));
            }
            _ => {
                let mut acro = Dictionary::new();
                add_field(&mut acro, &doc, &mut upd);
                let aid = alloc();
                upd.insert(aid, Object::Dictionary(acro));
                let mut c = cat.clone();
                c.set("AcroForm", Object::Reference(aid));
                upd.insert(root_id, Object::Dictionary(c));
            }
        }
    }

    // ---- Ghi incremental update ----
    let now = time_now_secs();
    let m_date = pdf_date(now, req.tz_offset_min);
    let mut out = base;
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut offsets: BTreeMap<u32, (usize, u16)> = BTreeMap::new();
    let mut br_span = (0usize, 0usize);
    let mut contents_span = (0usize, 0usize); // vị trí '<' và '>'
    for (oid, obj) in &upd {
        offsets.insert(oid.0, (out.len(), oid.1));
        out.extend_from_slice(format!("{} {} obj\n", oid.0, oid.1).as_bytes());
        if *oid == sig_id {
            out.extend_from_slice(b"<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached /ByteRange ");
            let s = out.len();
            out.extend_from_slice(BYTERANGE_PLACEHOLDER.as_bytes());
            br_span = (s, out.len());
            out.extend_from_slice(b"\n/Contents <");
            let lt = out.len() - 1;
            out.extend(std::iter::repeat(b'0').take(CONTENTS_CAPACITY * 2));
            contents_span = (lt, out.len());
            out.extend_from_slice(b">\n/M ");
            write_obj(&mut out, &Object::string_literal(m_date.clone()));
            out.extend_from_slice(b" /Name ");
            write_obj(&mut out, &text_obj(&display_name));
            if !req.reason.trim().is_empty() {
                out.extend_from_slice(b" /Reason ");
                write_obj(&mut out, &text_obj(req.reason.trim()));
            }
            if !req.location.trim().is_empty() {
                out.extend_from_slice(b" /Location ");
                write_obj(&mut out, &text_obj(req.location.trim()));
            }
            if !req.contact_info.trim().is_empty() {
                out.extend_from_slice(b" /ContactInfo ");
                write_obj(&mut out, &text_obj(req.contact_info.trim()));
            }
            out.extend_from_slice(b"\n/Prop_Build << /App << /Name /FoFreeXit >> /Filter << /Name /Adobe.PPKLite >> >> >>");
        } else {
            write_obj(&mut out, obj);
        }
        out.extend_from_slice(b"\nendobj\n");
    }

    // Trailer: giữ /Root /Info /ID của bản gốc, /Prev → xref cũ.
    let mut trailer = Dictionary::new();
    trailer.set("Root", Object::Reference(root_id));
    if let Ok(i) = doc.trailer.get(b"Info") {
        trailer.set("Info", i.clone());
    }
    if let Ok(idarr) = doc.trailer.get(b"ID") {
        trailer.set("ID", idarr.clone());
    }
    trailer.set("Prev", Object::Integer(info.startxref as i64));
    let xref_pos = out.len();
    if info.xref_stream {
        // Xref stream (không nén) — bản gốc dùng xref stream thì bản cập nhật cũng vậy.
        let xs_id = next_id;
        next_id += 1;
        offsets.insert(xs_id, (xref_pos, 0));
        let mut rows = Vec::new();
        let mut index = Vec::new();
        for (start, run) in runs(&offsets) {
            index.push(Object::Integer(start as i64));
            index.push(Object::Integer(run.len() as i64));
            for (off, gen) in run {
                rows.push(1u8);
                rows.extend_from_slice(&(off as u32).to_be_bytes());
                rows.extend_from_slice(&gen.to_be_bytes());
            }
        }
        trailer.set("Type", Object::Name(b"XRef".to_vec()));
        trailer.set("Size", Object::Integer(next_id as i64));
        trailer.set("W", Object::Array(vec![1.into(), 4.into(), 2.into()]));
        trailer.set("Index", Object::Array(index));
        out.extend_from_slice(format!("{xs_id} 0 obj\n").as_bytes());
        write_obj(&mut out, &Object::Stream(lopdf::Stream::new(trailer, rows)));
        out.extend_from_slice(b"\nendobj\n");
    } else {
        out.extend_from_slice(b"xref\n");
        for (start, run) in runs(&offsets) {
            out.extend_from_slice(format!("{start} {}\n", run.len()).as_bytes());
            for (off, gen) in run {
                out.extend_from_slice(format!("{off:010} {gen:05} n\r\n").as_bytes());
            }
        }
        trailer.set("Size", Object::Integer(next_id as i64));
        out.extend_from_slice(b"trailer\n");
        write_obj(&mut out, &Object::Dictionary(trailer));
        out.push(b'\n');
    }
    out.extend_from_slice(format!("startxref\n{xref_pos}\n%%EOF\n").as_bytes());

    // ---- ByteRange thật + băm + CMS ----
    let (c_lt, c_gt) = contents_span;
    let a1 = c_lt + 1; // đoạn 1 = [0, a1) (gồm '<')
    let b0 = c_gt; // đoạn 2 bắt đầu tại '>'
    let b1 = out.len() - c_gt;
    let real = format!("[0 {a1} {b0} {b1}]");
    if real.len() > br_span.1 - br_span.0 {
        return Err(fail("ByteRange thật dài hơn placeholder"));
    }
    write_padded(&mut out, br_span.0, br_span.1, real.as_bytes());

    let mut hasher = Sha256::new();
    hasher.update(&out[..a1]);
    hasher.update(&out[b0..b0 + b1]);
    let digest = hasher.finalize();

    let cms_der = build_cms(&id.cert_chain, &id.key, &digest)?;
    if cms_der.len() * 2 > CONTENTS_CAPACITY * 2 {
        return Err(fail(format!("CMS ({} byte) vượt sức chứa Contents", cms_der.len())));
    }
    let hex = to_hex(&cms_der);
    out[a1..a1 + hex.len()].copy_from_slice(hex.as_bytes());

    std::fs::write(output, &out)?;
    Ok(())
}

/// Gom offset theo dải id liên tiếp (subsection xref).
fn runs(offsets: &BTreeMap<u32, (usize, u16)>) -> Vec<(u32, Vec<(usize, u16)>)> {
    let mut out: Vec<(u32, Vec<(usize, u16)>)> = Vec::new();
    for (&id, &v) in offsets {
        match out.last_mut() {
            Some((start, run)) if *start + run.len() as u32 == id => run.push(v),
            _ => out.push((id, vec![v])),
        }
    }
    out
}

/// Dựng Form XObject giao diện chữ ký kích thước w×h: trái = ảnh chữ ký
/// (hoặc chữ lớn), phải = các dòng chi tiết. Chữ Unicode (tiếng Việt…) vẽ
/// qua PDFium + font nhúng của engine (như Thêm text), rồi chép nội dung +
/// tài nguyên trang tạm vào tài liệu đích dưới dạng Form XObject.
fn build_appearance(
    pdfium: &pdfium_render::prelude::Pdfium,
    w: f32,
    h: f32,
    ap: &SigAppearance,
    alloc: &mut impl FnMut() -> ObjectId,
    upd: &mut BTreeMap<ObjectId, Object>,
) -> Result<ObjectId, EngineError> {
    use pdfium_render::prelude::*;
    let stamp = format!("{}_{}", std::process::id(), time_nanos());
    let blank = std::env::temp_dir().join(format!("ff_sigap_blank_{stamp}.pdf"));
    let drawn = std::env::temp_dir().join(format!("ff_sigap_{stamp}.pdf"));
    {
        let mut d = pdfium.create_new_pdf().map_err(eng("tạo trang tạm"))?;
        d.pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(w), PdfPoints::new(h)))
            .map_err(eng("tạo trang tạm"))?;
        d.save_to_file(&blank).map_err(eng("lưu trang tạm"))?;
    }

    let pad = (h * 0.06).clamp(1.0, 6.0);
    let has_left = ap.image_path.is_some() || !ap.big_text.trim().is_empty();
    let lines: Vec<&String> = ap.lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let (left_w, right_x) = if has_left && !lines.is_empty() {
        (w * 0.5, w * 0.5 + pad)
    } else if has_left {
        (w, w)
    } else {
        (0.0, pad)
    };
    let mut ops: Vec<crate::EditOp> = Vec::new();

    // Trái: ảnh (giữ tỉ lệ, căn giữa) hoặc chữ lớn.
    if let Some(img_path) = &ap.image_path {
        let (iw, ih) = image::image_dimensions(img_path).map_err(eng("đọc ảnh chữ ký"))?;
        let (bw, bh) = (left_w - 2.0 * pad, h - 2.0 * pad);
        if bw > 1.0 && bh > 1.0 && iw > 0 && ih > 0 {
            let s = (bw / iw as f32).min(bh / ih as f32);
            let (dw, dh) = (iw as f32 * s, ih as f32 * s);
            ops.push(crate::EditOp::AddImage {
                x: pad + (bw - dw) / 2.0,
                y: pad + (bh - dh) / 2.0,
                width_pt: dw,
                height_pt: dh,
                image_path: img_path.clone(),
            });
        }
    } else if !ap.big_text.trim().is_empty() {
        let t = ap.big_text.trim();
        let n = t.chars().count().max(1) as f32;
        let size = ((left_w - 2.0 * pad) / (n * 0.55)).min((h - 2.0 * pad) * 0.6).max(4.0);
        ops.push(crate::EditOp::AddText {
            x: pad,
            y: (h - size * 0.72) / 2.0,
            text: t.to_string(),
            font_size: size,
            color: [0, 0, 0, 255],
            font_family: None,
            bold: false,
            italic: false,
        });
    }

    // Phải: dòng chi tiết, cỡ chữ vừa khung (ước lượng 0.55em/ký tự).
    if !lines.is_empty() {
        let avail_w = (w - right_x - pad).max(4.0);
        let maxc = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as f32;
        let size = (avail_w / (maxc * 0.52))
            .min((h - 2.0 * pad) / (lines.len() as f32 * 1.22))
            .clamp(2.0, 14.0);
        let lead = size * 1.22;
        let block = lead * lines.len() as f32;
        let top = h - (h - block) / 2.0;
        for (i, l) in lines.iter().enumerate() {
            ops.push(crate::EditOp::AddText {
                x: right_x,
                y: top - lead * (i as f32 + 1.0) + (lead - size) * 0.5 + size * 0.2,
                text: l.trim().to_string(),
                font_size: size,
                color: [0, 0, 0, 255],
                font_family: None,
                bold: false,
                italic: false,
            });
        }
    }

    let src_path = if ops.is_empty() {
        blank.clone()
    } else {
        crate::edit::apply_edits(pdfium, &blank, 0, &ops, &drawn, None)?;
        drawn.clone()
    };
    let src = Document::load(&src_path).map_err(eng("đọc giao diện tạm"));
    let _ = std::fs::remove_file(&blank);
    let _ = std::fs::remove_file(&drawn);
    let src = src?;
    let pid = *src.get_pages().values().next().ok_or_else(|| fail("trang tạm rỗng"))?;
    let content = src.get_page_content(pid).map_err(eng("nội dung giao diện"))?;
    let resources = src
        .get_dictionary(pid)
        .ok()
        .and_then(|p| p.get(b"Resources").ok())
        .map(|r| resolve(&src, r).clone())
        .unwrap_or(Object::Dictionary(Dictionary::new()));

    let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
    let res = import_obj(&src, &resources, &mut map, alloc, upd);
    let mut xd = Dictionary::new();
    xd.set("Type", Object::Name(b"XObject".to_vec()));
    xd.set("Subtype", Object::Name(b"Form".to_vec()));
    xd.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]));
    xd.set("Resources", res);
    let xid = alloc();
    upd.insert(xid, Object::Stream(lopdf::Stream::new(xd, content)));
    Ok(xid)
}

/// Chép sâu object từ tài liệu khác (cấp id mới cho mọi tham chiếu).
fn import_obj(
    src: &Document,
    obj: &Object,
    map: &mut HashMap<ObjectId, ObjectId>,
    alloc: &mut impl FnMut() -> ObjectId,
    upd: &mut BTreeMap<ObjectId, Object>,
) -> Object {
    match obj {
        Object::Reference(r) => {
            if let Some(n) = map.get(r) {
                return Object::Reference(*n);
            }
            let n = alloc();
            map.insert(*r, n);
            let target = src.get_object(*r).cloned().unwrap_or(Object::Null);
            let copied = import_obj(src, &target, map, alloc, upd);
            upd.insert(n, copied);
            Object::Reference(n)
        }
        Object::Array(a) => Object::Array(a.iter().map(|o| import_obj(src, o, map, alloc, upd)).collect()),
        Object::Dictionary(d) => {
            let mut nd = Dictionary::new();
            for (k, v) in d.iter() {
                nd.set(k.clone(), import_obj(src, v, map, alloc, upd));
            }
            Object::Dictionary(nd)
        }
        Object::Stream(s) => {
            let mut nd = Dictionary::new();
            for (k, v) in s.dict.iter() {
                nd.set(k.clone(), import_obj(src, v, map, alloc, upd));
            }
            let mut ns = lopdf::Stream::new(nd, s.content.clone());
            ns.allows_compression = false;
            Object::Stream(ns)
        }
        o => o.clone(),
    }
}

// ---- Serializer object PDF tối giản (cho incremental update) ----

fn write_obj(out: &mut Vec<u8>, o: &Object) {
    match o {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Object::Real(r) => out.extend_from_slice(fmt_real(*r).as_bytes()),
        Object::Name(n) => write_name(out, n),
        Object::String(s, StringFormat::Hexadecimal) => {
            out.push(b'<');
            out.extend_from_slice(to_hex(s).as_bytes());
            out.push(b'>');
        }
        Object::String(s, StringFormat::Literal) => {
            out.push(b'(');
            for &b in s {
                match b {
                    b'(' | b')' | b'\\' => {
                        out.push(b'\\');
                        out.push(b);
                    }
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    _ => out.push(b),
                }
            }
            out.push(b')');
        }
        Object::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_obj(out, x);
            }
            out.push(b']');
        }
        Object::Dictionary(d) => write_dict(out, d),
        Object::Stream(s) => {
            let mut d = s.dict.clone();
            d.set("Length", Object::Integer(s.content.len() as i64));
            write_dict(out, &d);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&s.content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((n, g)) => out.extend_from_slice(format!("{n} {g} R").as_bytes()),
    }
}

fn write_dict(out: &mut Vec<u8>, d: &Dictionary) {
    out.extend_from_slice(b"<<");
    for (k, v) in d.iter() {
        out.push(b' ');
        write_name(out, k);
        out.push(b' ');
        write_obj(out, v);
    }
    out.extend_from_slice(b" >>");
}

fn write_name(out: &mut Vec<u8>, n: &[u8]) {
    out.push(b'/');
    for &b in n {
        let regular = (0x21..=0x7e).contains(&b) && !b"()<>[]{}/%#".contains(&b);
        if regular {
            out.push(b);
        } else {
            out.extend_from_slice(format!("#{b:02X}").as_bytes());
        }
    }
}

fn fmt_real(r: f32) -> String {
    if !r.is_finite() {
        return "0".into();
    }
    let s = format!("{r:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" { "0".into() } else { s.to_string() }
}

// =====================================================================
// Xác thực
// =====================================================================

/// Chi tiết chứng chỉ người ký.
#[derive(Clone, Debug, Default)]
pub struct CertDetails {
    pub subject: String,
    pub issuer: String,
    pub serial: String,
    pub not_before: String,
    pub not_after: String,
    pub not_before_secs: u64,
    pub not_after_secs: u64,
    pub self_signed: bool,
    pub key_bits: usize,
}

/// Kết quả xác thực 1 chữ ký.
#[derive(Clone, Debug, Default)]
pub struct SignatureCheck {
    pub signer: String,
    /// Chữ ký RSA hợp lệ trên signed attributes.
    pub crypto_valid: bool,
    /// messageDigest trong signed attrs khớp digest băm lại từ file.
    pub digest_matches: bool,
    /// Chữ ký phủ TOÀN BỘ file (không bị thêm nội dung sau khi ký).
    pub covers_document: bool,
    /// Phần thêm sau khi ký CHỈ gồm chữ ký khác (ký nhiều lần hợp lệ).
    pub later_changes_are_signatures: bool,
    /// /Name, /Reason, /Location, /ContactInfo, /M của signature dict.
    pub name: String,
    pub reason: String,
    pub location: String,
    pub contact_info: String,
    /// Thời gian ký dạng "YYYY-MM-DD HH:MM:SS ±HH:MM" (signingTime hoặc /M).
    pub sign_time: String,
    pub sub_filter: String,
    pub cert: CertDetails,
    /// Số chứng chỉ trong CMS (chuỗi).
    pub chain_len: usize,
    /// Chứng chỉ còn hiệu lực tại thời điểm ký.
    pub cert_valid_at_signing: bool,
    /// Thứ tự revision (1 = chữ ký đầu tiên).
    pub revision: usize,
}

impl SignatureCheck {
    /// Hợp lệ VÀ tài liệu không bị sửa sau khi ký.
    pub fn is_valid(&self) -> bool {
        self.crypto_valid && self.digest_matches && self.covers_document
    }
    /// Phần được ký còn nguyên vẹn (kể cả khi sau đó có thêm revision).
    pub fn intact(&self) -> bool {
        self.crypto_valid && self.digest_matches
    }
}

/// Xác thực mọi chữ ký trong `input`.
pub fn verify_signatures(input: &Path) -> Result<Vec<SignatureCheck>, EngineError> {
    let bytes = std::fs::read(input)?;
    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(br_pos) = find_from(&bytes, b"/ByteRange", search_from) {
        search_from = br_pos + 10;
        let Some((nums, _arr_end)) = parse_int_array(&bytes, br_pos + b"/ByteRange".len()) else {
            continue;
        };
        if nums.len() != 4 || nums.iter().any(|n| *n < 0) {
            continue;
        }
        let (a0, a1, b0, b1) = (nums[0] as usize, nums[1] as usize, nums[2] as usize, nums[3] as usize);
        if a0 + a1 > bytes.len() || b0 + b1 > bytes.len() || a1 > b0 {
            continue;
        }
        // Khoảng loại ra [a1, b0) chính là hex Contents giữa '<' và '>'.
        let hex = &bytes[a1..b0];
        let hex = hex.strip_prefix(b"<").unwrap_or(hex);
        let hex = hex.strip_suffix(b">").unwrap_or(hex);
        let Some(mut cms_der) = from_hex(hex) else { continue };
        if let Some(n) = der_total_len(&cms_der) {
            cms_der.truncate(n);
        }

        let covers_document = b0 + b1 == bytes.len();
        let meta = sig_dict_meta(&bytes, br_pos, b0);
        let tail = &bytes[(b0 + b1).min(bytes.len())..];
        let later_changes_are_signatures = !covers_document && find_from(tail, b"/ByteRange", 0).is_some();

        let mut check = match verify_cms(&cms_der, &bytes[a0..a0 + a1], &bytes[b0..b0 + b1]) {
            Ok(c) => c,
            Err(_) => SignatureCheck { signer: "(không đọc được)".into(), ..Default::default() },
        };
        check.covers_document = covers_document;
        check.later_changes_are_signatures = later_changes_are_signatures;
        check.name = meta.get("Name").cloned().unwrap_or_default();
        check.reason = meta.get("Reason").cloned().unwrap_or_default();
        check.location = meta.get("Location").cloned().unwrap_or_default();
        check.contact_info = meta.get("ContactInfo").cloned().unwrap_or_default();
        check.sub_filter = meta.get("SubFilter").cloned().unwrap_or_default();
        if check.sign_time.is_empty() {
            check.sign_time = meta.get("M").map(|m| pdf_date_display(m)).unwrap_or_default();
        }
        if check.signer.is_empty() || check.signer.starts_with('(') {
            if !check.name.is_empty() {
                check.signer = check.name.clone();
            }
        }
        check.revision = out.len() + 1;
        out.push(check);
    }
    Ok(out)
}

/// Đọc các khoá text của signature dict chứa `/ByteRange` ở `br_pos`
/// (vùng từ " obj" gần nhất phía trước tới "endobj" sau Contents).
fn sig_dict_meta(bytes: &[u8], br_pos: usize, contents_end: usize) -> HashMap<String, String> {
    let from = rfind(&bytes[br_pos.saturating_sub(4096)..br_pos], b"obj")
        .map(|p| p + br_pos.saturating_sub(4096))
        .unwrap_or(br_pos.saturating_sub(4096));
    let to = find_from(bytes, b"endobj", contents_end)
        .unwrap_or(bytes.len())
        .min(contents_end + 16384)
        .min(bytes.len());
    let win = &bytes[from..to];
    let mut m = HashMap::new();
    for key in ["Name", "Reason", "Location", "ContactInfo", "M"] {
        let needle = format!("/{key}");
        let mut i = 0;
        while let Some(p) = find_from(win, needle.as_bytes(), i) {
            i = p + needle.len();
            // Đúng tên khoá (không phải tiền tố của khoá dài hơn, vd /Name trong /Prop_Build).
            if win.get(i).map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
                continue;
            }
            let mut j = i;
            while j < win.len() && win[j].is_ascii_whitespace() {
                j += 1;
            }
            if let Some(s) = parse_pdf_string(win, j) {
                m.insert(key.to_string(), pdf_text(&s));
                break;
            }
        }
    }
    // SubFilter là Name.
    if let Some(p) = find_from(win, b"/SubFilter", 0) {
        let mut j = p + 10;
        while j < win.len() && win[j].is_ascii_whitespace() {
            j += 1;
        }
        if win.get(j) == Some(&b'/') {
            let st = j + 1;
            let mut e = st;
            while e < win.len() && !win[e].is_ascii_whitespace() && !b"/<>[]()".contains(&win[e]) {
                e += 1;
            }
            m.insert("SubFilter".into(), String::from_utf8_lossy(&win[st..e]).into_owned());
        }
    }
    m
}

/// Parse chuỗi PDF (literal có escape, hoặc hex) bắt đầu tại `i`.
fn parse_pdf_string(b: &[u8], i: usize) -> Option<Vec<u8>> {
    match b.get(i)? {
        b'(' => {
            let mut out = Vec::new();
            let mut depth = 1;
            let mut j = i + 1;
            while j < b.len() {
                let c = b[j];
                match c {
                    b'\\' => {
                        j += 1;
                        let e = *b.get(j)?;
                        match e {
                            b'n' => out.push(b'\n'),
                            b'r' => out.push(b'\r'),
                            b't' => out.push(b'\t'),
                            b'b' => out.push(8),
                            b'f' => out.push(12),
                            b'0'..=b'7' => {
                                let mut v = (e - b'0') as u32;
                                for _ in 0..2 {
                                    match b.get(j + 1) {
                                        Some(d @ b'0'..=b'7') => {
                                            v = v * 8 + (*d - b'0') as u32;
                                            j += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                out.push(v as u8);
                            }
                            b'\r' | b'\n' => {}
                            other => out.push(other),
                        }
                    }
                    b'(' => {
                        depth += 1;
                        out.push(c);
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(out);
                        }
                        out.push(c);
                    }
                    _ => out.push(c),
                }
                j += 1;
            }
            None
        }
        b'<' if b.get(i + 1) != Some(&b'<') => {
            let end = find_from(b, b">", i)?;
            let mut h: Vec<u8> = b[i + 1..end].iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
            if h.len() % 2 == 1 {
                h.push(b'0');
            }
            from_hex(&h)
        }
        _ => None,
    }
}

// ---- CMS build/verify ----

fn build_cms(chain: &[Vec<u8>], priv_key: &RsaPrivateKey, digest: &[u8]) -> Result<Vec<u8>, EngineError> {
    use cms::builder::{create_signing_time_attribute, SignedDataBuilder, SignerInfoBuilder};
    use cms::cert::CertificateChoices;
    use cms::content_info::ContentInfo;
    use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
    use spki::AlgorithmIdentifierOwned;
    use x509_cert::Certificate;

    let cert = Certificate::from_der(&chain[0]).map_err(eng("parse cert"))?;
    let sid = SignerIdentifier::IssuerAndSerialNumber(cms::cert::IssuerAndSerialNumber {
        issuer: cert.tbs_certificate.issuer.clone(),
        serial_number: cert.tbs_certificate.serial_number.clone(),
    });
    let econtent = EncapsulatedContentInfo { econtent_type: const_oid::db::rfc5911::ID_DATA, econtent: None };
    let digest_algorithm = AlgorithmIdentifierOwned { oid: const_oid::db::rfc5912::ID_SHA_256, parameters: None };

    let signing_key = SigningKey::<Sha256>::new(priv_key.clone());
    let mut signer_info_builder =
        SignerInfoBuilder::new(&signing_key, sid, digest_algorithm.clone(), &econtent, Some(digest))
            .map_err(eng("signer info builder"))?;
    if let Ok(attr) = create_signing_time_attribute() {
        signer_info_builder.add_signed_attribute(attr).map_err(eng("signingTime"))?;
    }

    let mut builder = SignedDataBuilder::new(&econtent);
    builder.add_digest_algorithm(digest_algorithm).map_err(eng("add digest alg"))?;
    for c in chain {
        let x = Certificate::from_der(c).map_err(eng("parse cert chuỗi"))?;
        builder.add_certificate(CertificateChoices::Certificate(x)).map_err(eng("add cert"))?;
    }
    let content_info: ContentInfo = builder
        .add_signer_info::<SigningKey<Sha256>, rsa::pkcs1v15::Signature>(signer_info_builder)
        .map_err(eng("add signer info"))?
        .build()
        .map_err(eng("build cms"))?;
    content_info.to_der().map_err(eng("encode cms"))
}

/// Thuật toán băm theo OID.
#[derive(Clone, Copy)]
enum HashAlg {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

fn hash_alg(oid: &const_oid::ObjectIdentifier) -> Option<HashAlg> {
    match oid.to_string().as_str() {
        "1.3.14.3.2.26" => Some(HashAlg::Sha1),
        "2.16.840.1.101.3.4.2.1" => Some(HashAlg::Sha256),
        "2.16.840.1.101.3.4.2.2" => Some(HashAlg::Sha384),
        "2.16.840.1.101.3.4.2.3" => Some(HashAlg::Sha512),
        _ => None,
    }
}

fn hash_two(alg: HashAlg, a: &[u8], b: &[u8]) -> Vec<u8> {
    fn h<D: Digest>(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut d = D::new();
        d.update(a);
        d.update(b);
        d.finalize().to_vec()
    }
    match alg {
        HashAlg::Sha1 => h::<sha1::Sha1>(a, b),
        HashAlg::Sha256 => h::<Sha256>(a, b),
        HashAlg::Sha384 => h::<sha2::Sha384>(a, b),
        HashAlg::Sha512 => h::<sha2::Sha512>(a, b),
    }
}

fn verify_cms(cms_der: &[u8], part1: &[u8], part2: &[u8]) -> Result<SignatureCheck, EngineError> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::{SignedData, SignerIdentifier};

    let ci = ContentInfo::from_der(cms_der).map_err(eng("parse ContentInfo"))?;
    let sd = ci.content.decode_as::<SignedData>().map_err(eng("decode SignedData"))?;
    let certs: Vec<x509_cert::Certificate> = sd
        .certificates
        .as_ref()
        .map(|set| {
            set.0
                .iter()
                .filter_map(|c| match c {
                    cms::cert::CertificateChoices::Certificate(c) => Some(c.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let signer_info = sd
        .signer_infos
        .0
        .as_ref()
        .iter()
        .next()
        .ok_or_else(|| fail("CMS thiếu signerInfo"))?;
    // Cert người ký = cert khớp issuer+serial của SignerIdentifier.
    let cert = match &signer_info.sid {
        SignerIdentifier::IssuerAndSerialNumber(isn) => certs
            .iter()
            .find(|c| c.tbs_certificate.serial_number == isn.serial_number && c.tbs_certificate.issuer == isn.issuer)
            .or(certs.first()),
        _ => certs.first(),
    }
    .cloned()
    .ok_or_else(|| fail("CMS thiếu certificate"))?;
    let signer = common_name(&cert).unwrap_or_else(|| "(không rõ)".into());

    let alg = hash_alg(&signer_info.digest_alg.oid).unwrap_or(HashAlg::Sha256);
    let digest = hash_two(alg, part1, part2);

    let mut digest_matches = false;
    let mut sign_secs: Option<u64> = None;
    if let Some(attrs) = signer_info.signed_attrs.as_ref() {
        for attr in attrs.iter() {
            if attr.oid == const_oid::db::rfc5911::ID_MESSAGE_DIGEST {
                if let Some(v) = attr.values.iter().next() {
                    if let Ok(os) = v.decode_as::<der::asn1::OctetString>() {
                        digest_matches = os.as_bytes() == digest.as_slice();
                    }
                }
            } else if attr.oid == const_oid::db::rfc5911::ID_SIGNING_TIME {
                if let Some(v) = attr.values.iter().next() {
                    if let Ok(t) = v.to_der().and_then(|d| x509_cert::time::Time::from_der(&d)) {
                        sign_secs = Some(t.to_unix_duration().as_secs());
                    }
                }
            }
        }
    }
    let crypto_valid = verify_signed_attrs(&cert, signer_info, alg).unwrap_or(false);
    let details = cert_details(&cert);
    let when = sign_secs.unwrap_or_else(time_now_secs);
    let cert_valid_at_signing = when >= details.not_before_secs && when <= details.not_after_secs;
    Ok(SignatureCheck {
        signer,
        crypto_valid,
        digest_matches,
        sign_time: sign_secs.map(|s| fmt_utc(s)).unwrap_or_default(),
        cert: details,
        chain_len: certs.len(),
        cert_valid_at_signing,
        ..Default::default()
    })
}

fn verify_signed_attrs(
    cert: &x509_cert::Certificate,
    signer_info: &cms::signed_data::SignerInfo,
    alg: HashAlg,
) -> Option<bool> {
    use rsa::pkcs1v15::Signature;
    use rsa::RsaPublicKey;
    use spki::DecodePublicKey;

    let attrs = signer_info.signed_attrs.as_ref()?;
    // Dữ liệu ký = DER của signed attributes dưới tag SET OF (0x31).
    let signed_der = attrs.to_der().ok()?;
    let spki_der = cert.tbs_certificate.subject_public_key_info.to_der().ok()?;
    let pubkey = RsaPublicKey::from_public_key_der(&spki_der).ok()?;
    let sig = Signature::try_from(signer_info.signature.as_bytes()).ok()?;
    Some(match alg {
        HashAlg::Sha1 => VerifyingKey::<sha1::Sha1>::new(pubkey).verify(&signed_der, &sig).is_ok(),
        HashAlg::Sha256 => VerifyingKey::<Sha256>::new(pubkey).verify(&signed_der, &sig).is_ok(),
        HashAlg::Sha384 => VerifyingKey::<sha2::Sha384>::new(pubkey).verify(&signed_der, &sig).is_ok(),
        HashAlg::Sha512 => VerifyingKey::<sha2::Sha512>::new(pubkey).verify(&signed_der, &sig).is_ok(),
    })
}

fn cert_details(cert: &x509_cert::Certificate) -> CertDetails {
    let tbs = &cert.tbs_certificate;
    let nb = tbs.validity.not_before.to_unix_duration().as_secs();
    let na = tbs.validity.not_after.to_unix_duration().as_secs();
    let key_bits = {
        use rsa::traits::PublicKeyParts;
        use spki::DecodePublicKey;
        tbs.subject_public_key_info
            .to_der()
            .ok()
            .and_then(|d| rsa::RsaPublicKey::from_public_key_der(&d).ok())
            .map(|k| k.n().bits())
            .unwrap_or(0)
    };
    CertDetails {
        subject: tbs.subject.to_string(),
        issuer: tbs.issuer.to_string(),
        serial: to_hex(tbs.serial_number.as_bytes()).to_uppercase(),
        not_before: fmt_utc(nb),
        not_after: fmt_utc(na),
        not_before_secs: nb,
        not_after_secs: na,
        self_signed: tbs.subject == tbs.issuer,
        key_bits,
    }
}

fn common_name(cert: &x509_cert::Certificate) -> Option<String> {
    dn_attr(&cert.tbs_certificate.subject, "2.5.4.3")
}

fn cert_email(cert: &x509_cert::Certificate) -> Option<String> {
    // emailAddress trong DN (1.2.840.113549.1.9.1); SAN rfc822 bỏ qua cho gọn.
    dn_attr(&cert.tbs_certificate.subject, "1.2.840.113549.1.9.1")
}

fn dn_attr(name: &x509_cert::name::Name, oid: &str) -> Option<String> {
    for rdn in name.0.iter() {
        for atv in rdn.0.iter() {
            if atv.oid.to_string() == oid {
                if let Ok(s) = atv.value.decode_as::<der::asn1::Utf8StringRef>() {
                    return Some(s.to_string());
                }
                if let Ok(s) = atv.value.decode_as::<der::asn1::PrintableStringRef>() {
                    return Some(s.to_string());
                }
                if let Ok(s) = atv.value.decode_as::<der::asn1::Ia5StringRef>() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

// ---- byte helpers ----

fn to_hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

fn from_hex(hex: &[u8]) -> Option<Vec<u8>> {
    let clean: Vec<u8> = hex.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut v = Vec::with_capacity(clean.len() / 2);
    let n = clean.len() / 2 * 2;
    let mut i = 0;
    while i < n {
        let hi = hex_val(clean[i])?;
        let lo = hex_val(clean[i + 1])?;
        v.push((hi << 4) | lo);
        i += 2;
    }
    Some(v)
}

/// Độ dài tổng (header + nội dung) của phần tử DER đầu tiên.
fn der_total_len(der: &[u8]) -> Option<usize> {
    if der.len() < 2 {
        return None;
    }
    let len_byte = der[1];
    if len_byte < 0x80 {
        return Some(2 + len_byte as usize);
    }
    let num = (len_byte & 0x7f) as usize;
    if num == 0 || der.len() < 2 + num {
        return None;
    }
    let mut len = 0usize;
    for &b in &der[2..2 + num] {
        len = (len << 8) | b as usize;
    }
    Some(2 + num + len)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Ghi `data` vào [start,end), pad phần thừa bằng dấu cách.
fn write_padded(buf: &mut [u8], start: usize, end: usize, data: &[u8]) {
    for (i, slot) in (start..end).enumerate() {
        buf[slot] = if i < data.len() { data[i] } else { b' ' };
    }
}

/// Parse mảng số nguyên "[a b c d]" bắt đầu quét từ `from`. Trả (số, offset sau ']').
fn parse_int_array(buf: &[u8], from: usize) -> Option<(Vec<i64>, usize)> {
    let mut i = from;
    while i < buf.len() && buf[i] != b'[' {
        if !buf[i].is_ascii_whitespace() {
            return None;
        }
        i += 1;
    }
    if i >= buf.len() {
        return None;
    }
    i += 1;
    let mut nums = Vec::new();
    let mut cur = String::new();
    while i < buf.len() && buf[i] != b']' {
        let c = buf[i];
        if c.is_ascii_digit() || c == b'-' {
            cur.push(c as char);
        } else if !cur.is_empty() {
            nums.push(cur.parse().ok()?);
            cur.clear();
        }
        i += 1;
    }
    if !cur.is_empty() {
        nums.push(cur.parse().ok()?);
    }
    Some((nums, i + 1))
}

fn time_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn time_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// D:YYYYMMDDHHmmSS+HH'mm' theo giờ địa phương (lệch `tz_min` phút).
fn pdf_date(secs: u64, tz_min: i32) -> String {
    let local = secs as i64 + tz_min as i64 * 60;
    let (y, mo, d, h, mi, s) = civil(local);
    if tz_min == 0 {
        return format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z");
    }
    let sign = if tz_min < 0 { '-' } else { '+' };
    let a = tz_min.unsigned_abs();
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}{sign}{:02}'{:02}'", a / 60, a % 60)
}

/// "D:20260930101500+07'00'" → "2026-09-30 10:15:00 +07:00".
fn pdf_date_display(m: &str) -> String {
    let s = m.trim().trim_start_matches("D:");
    let d: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if d.len() < 8 {
        return m.to_string();
    }
    let g = |a: usize, b: usize| d.get(a..b).unwrap_or("00").to_string();
    let rest = &s[d.len()..];
    let tz = if rest.starts_with('Z') || rest.is_empty() {
        "UTC".to_string()
    } else {
        let t: String = rest.chars().filter(|c| c.is_ascii_digit() || *c == '+' || *c == '-').collect();
        if t.len() >= 5 { format!("{}:{}", &t[..3], &t[3..5]) } else { t }
    };
    format!("{}-{}-{} {}:{}:{} {}", g(0, 4), g(4, 6), g(6, 8), g(8, 10), g(10, 12), g(12, 14), tz)
}

fn fmt_utc(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs as i64);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

/// Epoch giây → (năm, tháng, ngày, giờ, phút, giây) UTC (thuật toán Howard Hinnant).
fn civil(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400) as u32;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, h, mi, s)
}
