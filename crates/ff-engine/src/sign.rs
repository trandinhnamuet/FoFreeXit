//! Chữ ký số PDF (Phase 5 iteration 2) — PAdES/PKCS#7 detached, RSA-SHA256.
//!
//! Ba việc:
//! 1. `generate_self_signed_id` — tạo Digital ID tự ký (RSA-2048 + X.509) ghi
//!    ra file PEM (cert + private key) để người dùng ký thử ngay, không cần
//!    mua CA. Người dùng có PFX/PEM thật thì nạp thẳng.
//! 2. `sign_pdf` — nhúng chữ ký:
//!    - Dựng signature dict (`/Type/Sig /SubFilter/adbe.pkcs7.detached`) với
//!      `/ByteRange` + `/Contents` PLACEHOLDER, gắn field chữ ký + AcroForm.
//!    - Serialize, tính offset thật của ByteRange/Contents, vá ByteRange.
//!    - SHA-256 trên toàn file TRỪ khoảng Contents → CMS SignedData detached
//!      (signed attrs gồm messageDigest + contentType) → nhét DER vào Contents.
//!    Đây đúng cơ chế Adobe: phần hex `<...>` của Contents bị loại khỏi digest,
//!    2 dấu `<` `>` vẫn nằm trong vùng ký.
//! 3. `verify_signatures` — với mỗi chữ ký: parse ByteRange/Contents, băm lại,
//!    kiểm CMS (chữ ký RSA trên signed attrs + messageDigest khớp digest) và
//!    xem chữ ký có phủ TOÀN BỘ file không (phát hiện sửa sau khi ký).

use std::path::Path;

use der::{Decode, Encode};
use rsa::pkcs1v15::{SigningKey, VerifyingKey};
use rsa::signature::Verifier;
use rsa::RsaPrivateKey;
use sha2::{Digest, Sha256};

use crate::qpdf;
use crate::EngineError;

/// Kích thước (byte) dành cho Contents chữ ký. CMS RSA-2048 tự ký ~2KB; 16KB
/// dư cho cả timestamp/chuỗi cert sau này.
const CONTENTS_CAPACITY: usize = 16384;

fn eng<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> EngineError + '_ {
    move |e| EngineError::Pdfium(format!("{ctx}: {e}"))
}

/// Tạo Digital ID tự ký, ghi ra `out_pem` (cert PEM + private key PEM nối nhau).
/// `common_name` là tên hiển thị trong chữ ký (vd "Nguyen Van A").
pub fn generate_self_signed_id(common_name: &str, out_pem: &Path) -> Result<(), EngineError> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_RSA_SHA256};

    let mut rng = rsa::rand_core::OsRng;
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).map_err(eng("tạo khoá RSA"))?;
    let signing_key = SigningKey::<Sha256>::new(priv_key);
    let key_pem = {
        use rsa::pkcs8::EncodePrivateKey;
        signing_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(eng("mã hoá PKCS#8"))?
            .to_string()
    };
    let key_pair = KeyPair::from_pkcs8_pem_and_sign_algo(&key_pem, &PKCS_RSA_SHA256)
        .map_err(eng("rcgen keypair"))?;

    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(eng("params"))?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn.push(DnType::OrganizationName, "FoFreeXit Self-Signed");
    params.distinguished_name = dn;
    let cert = params.self_signed(&key_pair).map_err(eng("tự ký cert"))?;

    let mut bundle = cert.pem();
    bundle.push('\n');
    bundle.push_str(&key_pair.serialize_pem());
    std::fs::write(out_pem, bundle)?;
    Ok(())
}

/// Nạp (cert DER, RSA private key) từ file PEM bundle (cert + key nối nhau,
/// hoặc chỉ 1 trong 2 loại). Tách từng khối PEM rồi decode DER đúng loại —
/// KHÔNG đưa cả bundle vào parser PEM (parser sẽ vấp khối CERTIFICATE đứng đầu).
fn load_identity(pem_path: &Path) -> Result<(Vec<u8>, RsaPrivateKey), EngineError> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    let text = std::fs::read_to_string(pem_path)?;
    let blocks = pem_iter(&text);

    let cert_der = blocks
        .iter()
        .find(|b| b.tag == "CERTIFICATE")
        .map(|b| b.der.clone())
        .ok_or_else(|| EngineError::Pdfium("PEM thiếu CERTIFICATE".into()))?;

    let key_block = blocks
        .iter()
        .find(|b| b.tag == "PRIVATE KEY" || b.tag == "RSA PRIVATE KEY")
        .ok_or_else(|| EngineError::Pdfium("PEM thiếu PRIVATE KEY".into()))?;
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

/// Parser PEM tối giản (không phụ thuộc crate ngoài): tách các khối
/// `-----BEGIN X-----` … `-----END X-----`, base64-decode phần giữa.
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

/// Ký `input` bằng identity ở `id_pem`, ghi ra `output`. `reason` là lý do ký.
pub fn sign_pdf(
    input: &Path,
    id_pem: &Path,
    reason: &str,
    signer_name: &str,
    output: &Path,
) -> Result<(), EngineError> {
    let (cert_der, priv_key) = load_identity(id_pem)?;

    // Chuẩn hoá qua qpdf để offset ổn định + lopdf đọc chắc chắn.
    let norm = std::env::temp_dir().join(format!("ff_sign_norm_{}.pdf", std::process::id()));
    qpdf::repair(input, &norm)?;
    let mut doc = lopdf::Document::load(&norm).map_err(eng("lopdf load"))?;
    let _ = std::fs::remove_file(&norm);

    // Placeholder độ rộng CỐ ĐỊNH để vá tại chỗ: ByteRange 4 số 10 chữ số.
    let byterange_placeholder = "[0 9999999999 9999999999 9999999999]";
    let contents_zeros = vec![0u8; CONTENTS_CAPACITY];

    let now = lopdf::Object::string_literal(pdf_date_now());
    let mut sig_dict = lopdf::Dictionary::new();
    sig_dict.set("Type", lopdf::Object::Name(b"Sig".to_vec()));
    sig_dict.set("Filter", lopdf::Object::Name(b"Adobe.PPKLite".to_vec()));
    sig_dict.set("SubFilter", lopdf::Object::Name(b"adbe.pkcs7.detached".to_vec()));
    sig_dict.set("Contents", lopdf::Object::String(contents_zeros, lopdf::StringFormat::Hexadecimal));
    // ByteRange: nhét chuỗi thô cố định qua Object::Name để giữ nguyên byte khi
    // serialize (lopdf ghi Name là "/..."; ta bù trừ khi vá — xem patch_byterange).
    sig_dict.set(
        "ByteRange",
        lopdf::Object::Reference((u32::MAX, 0)), // tạm; thay bằng chuỗi thô sau serialize
    );
    sig_dict.set("Reason", lopdf::Object::string_literal(reason.to_string()));
    sig_dict.set("Name", lopdf::Object::string_literal(signer_name.to_string()));
    sig_dict.set("M", now);
    let sig_id = doc.add_object(lopdf::Object::Dictionary(sig_dict));

    // Widget field chữ ký (vô hình) trên trang đầu.
    let page_id = *doc
        .get_pages()
        .values()
        .next()
        .ok_or_else(|| EngineError::Pdfium("PDF không có trang".into()))?;
    let mut widget = lopdf::Dictionary::new();
    widget.set("Type", lopdf::Object::Name(b"Annot".to_vec()));
    widget.set("Subtype", lopdf::Object::Name(b"Widget".to_vec()));
    widget.set("FT", lopdf::Object::Name(b"Sig".to_vec()));
    widget.set("T", lopdf::Object::string_literal("Signature1"));
    widget.set("V", lopdf::Object::Reference(sig_id));
    widget.set("F", lopdf::Object::Integer(132)); // Print+Locked
    widget.set(
        "Rect",
        lopdf::Object::Array(vec![0.into(), 0.into(), 0.into(), 0.into()]),
    );
    widget.set("P", lopdf::Object::Reference(page_id));
    let widget_id = doc.add_object(lopdf::Object::Dictionary(widget));

    // Gắn widget vào /Annots của trang.
    {
        let page = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut().map_err(Into::into));
        if let Ok(page) = page {
            match page.get(b"Annots").ok().and_then(|o| o.as_array().ok()) {
                Some(arr) => {
                    let mut arr = arr.clone();
                    arr.push(lopdf::Object::Reference(widget_id));
                    page.set("Annots", lopdf::Object::Array(arr));
                }
                None => {
                    page.set("Annots", lopdf::Object::Array(vec![lopdf::Object::Reference(widget_id)]));
                }
            }
        }
    }

    // AcroForm với /SigFlags 3 + field chữ ký.
    let catalog_id = doc
        .catalog()
        .map_err(eng("catalog"))?
        .get(b"Type")
        .map(|_| ())
        .and(doc.trailer.get(b"Root").and_then(|o| o.as_reference()))
        .map_err(eng("root id"))?;
    {
        let mut acro = lopdf::Dictionary::new();
        acro.set("Fields", lopdf::Object::Array(vec![lopdf::Object::Reference(widget_id)]));
        acro.set("SigFlags", lopdf::Object::Integer(3));
        let acro_id = doc.add_object(lopdf::Object::Dictionary(acro));
        if let Ok(cat) = doc.get_object_mut(catalog_id).and_then(|o| o.as_dict_mut().map_err(Into::into)) {
            cat.set("AcroForm", lopdf::Object::Reference(acro_id));
        }
    }

    // Serialize ra bytes, rồi vá ByteRange placeholder (đang là ref u32::MAX 0).
    let mut buf: Vec<u8> = Vec::new();
    doc.save_to(&mut buf).map_err(eng("serialize"))?;

    // Thay reference placeholder của ByteRange bằng chuỗi mảng thô.
    let buf = replace_byterange_ref(buf, byterange_placeholder)?;

    // Định vị Contents <...> và ByteRange [...] trong bytes.
    let (c_lt, c_gt) = find_contents_span(&buf)?;
    let (br_start, br_end) = find_span(&buf, byterange_placeholder.as_bytes())
        .ok_or_else(|| EngineError::Pdfium("không thấy ByteRange placeholder".into()))?;

    // ByteRange = [0, off_after_'<'... ]: vùng ký = [0, c_lt+1) ++ [c_gt, EOF).
    let a0 = 0usize;
    let a1 = c_lt + 1; // độ dài đoạn 1 (gồm '<')
    let b0 = c_gt; // bắt đầu đoạn 2 (gồm '>')
    let b1 = buf.len() - c_gt; // độ dài đoạn 2
    let real = format!("[{a0} {a1} {b0} {b1}]");
    if real.len() > (br_end - br_start) {
        return Err(EngineError::Pdfium("ByteRange thật dài hơn placeholder".into()));
    }
    let mut patched = buf;
    write_padded(&mut patched, br_start, br_end, real.as_bytes());

    // Băm SHA-256 trên 2 đoạn (loại phần hex giữa '<' '>').
    let mut hasher = Sha256::new();
    hasher.update(&patched[a0..a1]);
    hasher.update(&patched[b0..b0 + b1]);
    let digest = hasher.finalize();

    // Dựng CMS SignedData detached.
    let cms_der = build_cms(&cert_der, &priv_key, &digest)?;
    if cms_der.len() * 2 > (c_gt - (c_lt + 1)) {
        return Err(EngineError::Pdfium(format!(
            "CMS ({} byte) vượt sức chứa Contents",
            cms_der.len()
        )));
    }

    // Ghi hex CMS vào giữa '<' '>' (phần còn lại giữ '0').
    let hex = to_hex(&cms_der);
    let slot_start = c_lt + 1;
    for (i, b) in hex.bytes().enumerate() {
        patched[slot_start + i] = b;
    }

    std::fs::write(output, &patched)?;
    Ok(())
}

/// Kết quả xác thực 1 chữ ký.
#[derive(Clone, Debug)]
pub struct SignatureCheck {
    pub signer: String,
    /// Chữ ký RSA hợp lệ trên signed attributes.
    pub crypto_valid: bool,
    /// messageDigest trong signed attrs khớp digest băm lại từ file.
    pub digest_matches: bool,
    /// Chữ ký phủ TOÀN BỘ file (không bị thêm nội dung sau khi ký).
    pub covers_document: bool,
}

impl SignatureCheck {
    pub fn is_valid(&self) -> bool {
        self.crypto_valid && self.digest_matches && self.covers_document
    }
}

/// Xác thực mọi chữ ký trong `input`.
pub fn verify_signatures(input: &Path) -> Result<Vec<SignatureCheck>, EngineError> {
    let bytes = std::fs::read(input)?;
    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = find_from(&bytes, b"/ByteRange", search_from) {
        let br_pos = rel;
        search_from = br_pos + 10;
        // Parse mảng ByteRange sau /ByteRange.
        let Some((nums, _arr_end)) = parse_int_array(&bytes, br_pos + b"/ByteRange".len()) else {
            continue;
        };
        if nums.len() != 4 {
            continue;
        }
        let (a0, a1, b0, b1) = (nums[0] as usize, nums[1] as usize, nums[2] as usize, nums[3] as usize);
        if a0 + a1 > bytes.len() || b0 + b1 > bytes.len() || a1 > b0 {
            continue;
        }
        // Vùng ký = [a0, a1) ++ [b0, b0+b1); khoảng loại ra [a1, b0) chính là hex
        // Contents giữa '<' và '>' (không gồm 2 dấu — khớp cách ký ở sign_pdf).
        let hex = &bytes[a1..b0];
        let Some(mut cms_der) = from_hex(hex) else { continue };
        // Contents chứa CMS rồi padding '0' → cắt về đúng độ dài DER (SEQUENCE
        // ngoài cùng tự mô tả), nếu không parser DER sẽ chê dữ liệu thừa.
        if let Some(n) = der_total_len(&cms_der) {
            cms_der.truncate(n);
        }

        let mut hasher = Sha256::new();
        hasher.update(&bytes[a0..a0 + a1]);
        hasher.update(&bytes[b0..b0 + b1]);
        let digest = hasher.finalize();

        let covers_document = b0 + b1 == bytes.len();
        let check = verify_cms(&cms_der, &digest, covers_document)
            .unwrap_or(SignatureCheck {
                signer: "(không đọc được)".into(),
                crypto_valid: false,
                digest_matches: false,
                covers_document,
            });
        out.push(check);
    }
    Ok(out)
}

// ---- CMS build/verify ----

fn build_cms(cert_der: &[u8], priv_key: &RsaPrivateKey, digest: &[u8]) -> Result<Vec<u8>, EngineError> {
    use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
    use cms::cert::CertificateChoices;
    use cms::content_info::ContentInfo;
    use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
    use spki::AlgorithmIdentifierOwned;
    use x509_cert::Certificate;

    let cert = Certificate::from_der(cert_der).map_err(eng("parse cert"))?;
    let sid = SignerIdentifier::IssuerAndSerialNumber(cms::cert::IssuerAndSerialNumber {
        issuer: cert.tbs_certificate.issuer.clone(),
        serial_number: cert.tbs_certificate.serial_number.clone(),
    });

    // id-data (1.2.840.113549.1.7.1)
    let econtent = EncapsulatedContentInfo {
        econtent_type: const_oid::db::rfc5911::ID_DATA,
        econtent: None,
    };
    // SHA-256 AlgorithmIdentifier (2.16.840.1.101.3.4.2.1)
    let digest_algorithm = AlgorithmIdentifierOwned {
        oid: const_oid::db::rfc5912::ID_SHA_256,
        parameters: None,
    };

    let signing_key = SigningKey::<Sha256>::new(priv_key.clone());
    let signer_info_builder = SignerInfoBuilder::new(
        &signing_key,
        sid,
        digest_algorithm.clone(),
        &econtent,
        Some(digest),
    )
    .map_err(eng("signer info builder"))?;

    let content_info: ContentInfo = SignedDataBuilder::new(&econtent)
        .add_digest_algorithm(digest_algorithm)
        .map_err(eng("add digest alg"))?
        .add_certificate(CertificateChoices::Certificate(cert))
        .map_err(eng("add cert"))?
        .add_signer_info::<SigningKey<Sha256>, rsa::pkcs1v15::Signature>(signer_info_builder)
        .map_err(eng("add signer info"))?
        .build()
        .map_err(eng("build cms"))?;

    content_info.to_der().map_err(eng("encode cms"))
}

fn verify_cms(cms_der: &[u8], digest: &[u8], covers_document: bool) -> Result<SignatureCheck, EngineError> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;

    let ci = ContentInfo::from_der(cms_der).map_err(eng("parse ContentInfo"))?;
    let sd = ci.content.decode_as::<SignedData>().map_err(eng("decode SignedData"))?;

    // Cert đầu tiên.
    let cert = sd
        .certificates
        .as_ref()
        .and_then(|set| set.0.iter().next())
        .and_then(|choice| match choice {
            cms::cert::CertificateChoices::Certificate(c) => Some(c.clone()),
            _ => None,
        })
        .ok_or_else(|| EngineError::Pdfium("CMS thiếu certificate".into()))?;
    let signer = common_name(&cert).unwrap_or_else(|| "(không rõ)".into());

    let signer_info = sd
        .signer_infos
        .0
        .as_ref()
        .iter()
        .next()
        .ok_or_else(|| EngineError::Pdfium("CMS thiếu signerInfo".into()))?;

    // messageDigest signed attr khớp digest?
    let mut digest_matches = false;
    if let Some(attrs) = signer_info.signed_attrs.as_ref() {
        for attr in attrs.iter() {
            if attr.oid == const_oid::db::rfc5911::ID_MESSAGE_DIGEST {
                if let Some(v) = attr.values.iter().next() {
                    if let Ok(os) = v.decode_as::<der::asn1::OctetString>() {
                        digest_matches = os.as_bytes() == digest;
                    }
                }
            }
        }
    }

    // Chữ ký RSA trên DER(signed attrs) [IMPLICIT SET OF → EXPLICIT khi ký].
    let crypto_valid = verify_signed_attrs(&cert, signer_info).unwrap_or(false);

    Ok(SignatureCheck { signer, crypto_valid, digest_matches, covers_document })
}

fn verify_signed_attrs(
    cert: &x509_cert::Certificate,
    signer_info: &cms::signed_data::SignerInfo,
) -> Option<bool> {
    use rsa::pkcs1v15::Signature;
    use rsa::RsaPublicKey;
    use spki::DecodePublicKey;

    let attrs = signer_info.signed_attrs.as_ref()?;
    // Dữ liệu ký = DER của signed attributes dưới tag SET OF (0x31), không phải
    // [0] IMPLICIT của SignerInfo. der crate encode SignedAttributes ra đúng vậy.
    let signed_der = attrs.to_der().ok()?;

    let spki_der = cert.tbs_certificate.subject_public_key_info.to_der().ok()?;
    let pubkey = RsaPublicKey::from_public_key_der(&spki_der).ok()?;
    let verifying = VerifyingKey::<Sha256>::new(pubkey);

    let sig_bytes = signer_info.signature.as_bytes();
    let sig = Signature::try_from(sig_bytes).ok()?;
    Some(verifying.verify(&signed_der, &sig).is_ok())
}

fn common_name(cert: &x509_cert::Certificate) -> Option<String> {
    for rdn in cert.tbs_certificate.subject.0.iter() {
        for atv in rdn.0.iter() {
            // OID 2.5.4.3 = commonName
            if atv.oid.to_string() == "2.5.4.3" {
                if let Ok(s) = atv.value.decode_as::<der::asn1::Utf8StringRef>() {
                    return Some(s.to_string());
                }
                if let Ok(s) = atv.value.decode_as::<der::asn1::PrintableStringRef>() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

// ---- byte helpers ----

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn from_hex(hex: &[u8]) -> Option<Vec<u8>> {
    let clean: Vec<u8> = hex.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    // Bỏ đuôi '0' padding (số chẵn). CMS DER tự mô tả độ dài nên thừa '0' vô hại,
    // nhưng ta cắt về bội số 2 và loại cặp "00" đuôi để parse gọn.
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

/// Độ dài tổng (header + nội dung) của phần tử DER đầu tiên trong `der`. Đọc
/// tag (1 byte) + trường length (ngắn/dài). Trả None nếu không đủ byte.
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

/// Thay Object::Reference placeholder "(4294967295 0 R)" của ByteRange bằng
/// chuỗi mảng thô (giữ nguyên tổng độ dài không quan trọng vì đây là bước
/// TRƯỚC khi định vị; ta pad để mảng đủ rộng).
fn replace_byterange_ref(buf: Vec<u8>, placeholder: &str) -> Result<Vec<u8>, EngineError> {
    let needle = b"/ByteRange";
    let pos = find_span(&buf, needle).map(|(s, _)| s);
    let Some(pos) = pos else {
        return Err(EngineError::Pdfium("không thấy /ByteRange".into()));
    };
    // Sau "/ByteRange" là " 4294967295 0 R". Tìm 'R' kết thúc reference.
    let after = pos + needle.len();
    let mut i = after;
    while i < buf.len() && buf[i] != b'R' {
        i += 1;
    }
    if i >= buf.len() {
        return Err(EngineError::Pdfium("ByteRange ref hỏng".into()));
    }
    let ref_end = i + 1; // gồm 'R'
    let mut out = Vec::with_capacity(buf.len());
    out.extend_from_slice(&buf[..after]);
    out.push(b' ');
    out.extend_from_slice(placeholder.as_bytes());
    out.extend_from_slice(&buf[ref_end..]);
    Ok(out)
}

/// Tìm span [start,end) của `needle` trong `hay` (lần đầu).
fn find_span(hay: &[u8], needle: &[u8]) -> Option<(usize, usize)> {
    find_from(hay, needle, 0).map(|s| (s, s + needle.len()))
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Định vị Contents CỦA CHỮ KÝ: trả (offset của '<', offset của '>').
/// Trang cũng có khoá `/Contents N 0 R` (tham chiếu, không phải hex string) nên
/// phải chọn đúng `/Contents` mà giá trị NGAY SAU là hex string `<...>` (ký tự
/// sau `<` không phải `<` để loại `<<` mở dict).
fn find_contents_span(buf: &[u8]) -> Result<(usize, usize), EngineError> {
    let needle = b"/Contents";
    let mut from = 0usize;
    while let Some(pos) = find_from(buf, needle, from) {
        from = pos + needle.len();
        // Bỏ khoảng trắng sau /Contents.
        let mut i = pos + needle.len();
        while i < buf.len() && buf[i].is_ascii_whitespace() {
            i += 1;
        }
        // Giá trị hex string bắt đầu bằng '<' và KHÔNG phải '<<'.
        if i < buf.len() && buf[i] == b'<' && !(i + 1 < buf.len() && buf[i + 1] == b'<') {
            let lt = i;
            i += 1;
            while i < buf.len() && buf[i] != b'>' {
                i += 1;
            }
            if i >= buf.len() {
                return Err(EngineError::Pdfium("Contents thiếu '>'".into()));
            }
            return Ok((lt, i));
        }
        // Ngược lại: /Contents của trang (tham chiếu) → tìm tiếp.
    }
    Err(EngineError::Pdfium("không thấy /Contents hex của chữ ký".into()))
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
    i += 1; // qua '['
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

fn pdf_date_now() -> String {
    // D:YYYYMMDDHHmmSS — dùng UNIX epoch quy đổi UTC đơn giản (đủ cho /M).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_epoch(secs);
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z")
}

/// Quy đổi epoch giây → (năm, tháng, ngày, giờ, phút, giây) UTC (thuật toán Howard Hinnant).
fn civil_from_epoch(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = (secs % 86400) as u32;
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
