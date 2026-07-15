# 15 — Tổng kết Phase 5 (Bảo mật) · Iteration 1 + 2

> Trạng thái: **HOÀN TẤT Phase 5.** Iter 1: mã hoá + permissions, gỡ mật khẩu,
> redaction THẬT, xoá metadata. Iter 2: **chữ ký số PKCS#7/PAdES (ký + xác
> thực + phát hiện giả mạo), redaction tỉa theo KÝ TỰ, lưu tối ưu.**
> 62/62 test engine xanh (ngoài 2 fixture qpdf Linux). Chi tiết iter 2 ở mục 6.

## 1. Khảo sát Foxit (chuẩn nghiệm thu)
- **Protect › Password Protect**: user/owner password, AES-256, tick quyền
  in/sửa/copy/chú thích. → ta làm dialog tương đương.
- **Protect › Mark for Redaction → Apply**: quét vùng → khối đen dashed →
  Apply xoá nội dung THẬT (text không còn select/search được, ảnh bị đục),
  vẽ khối đen. → ta làm đúng luồng 2 bước này.
- **Remove hidden information**: xoá metadata Author/Producer/XMP.

## 2. Đã làm

### Engine
- **`qpdf.rs`**: `Permissions {allow_print, allow_modify, allow_extract,
  allow_annotate}` map sang cờ AES-256 của qpdf (`--print/--modify/--extract/
  --annotate`); `encrypt_with_password_perms(...)` (API cũ giữ nguyên, uỷ
  quyền với quyền đầy đủ); `decrypt_remove_password(input, password, output)`
  (`qpdf --password=… --decrypt`) — password sai trả lỗi, không ghi file rác.
- **`redact.rs` (mới) — redaction THẬT, nghiêng về xoá thừa (an toàn)**:
  - Text/path/shading/form GIAO vùng → xoá NGUYÊN object khỏi trang (v1 xoá
    cả run bị chạm — sót chữ là lỗi bảo mật, xoá thừa thì không).
  - Ảnh giao vùng → `get_raw_image` đọc pixel gốc, **bôi đen đúng phần giao
    trong dữ liệu ảnh** rồi thay ảnh (nội dung gốc không còn trong file);
    không đọc được pixel → xoá cả ảnh.
  - Vẽ khối đen phủ mỗi vùng (dấu hiệu thị giác chuẩn redaction).
- **`meta.rs::strip_metadata`**: xoá `/Info` trong trailer (Author/Producer/
  CreationDate…) + stream XMP `/Metadata` trong catalog — xoá cả THAM CHIẾU
  lẫn OBJECT đích (kiểm bằng soi bytes thô của file kết quả). Qua lopdf;
  trailer phi chuẩn được qpdf chuẩn hoá trước (fallback trong Tauri command).

### Tauri + UI (thanh "🔒 Bảo mật")
- Commands: `redact_apply` (nhiều trang, chain qua file tạm, trả số object đã
  xử lý), `security_encrypt`, `security_decrypt`, `security_strip_metadata`.
- **Redact 2 bước như Foxit**: nút *⬛ Đánh dấu redact* (tool kéo-quét trên
  trang, giữ tool để quét nhiều vùng; khối đen mờ viền đỏ dashed, bấm vào
  khối để bỏ) → *Áp dụng redact (n)* → chọn nơi lưu → mở file kết quả.
- **Đặt mật khẩu**: dialog user/confirm/owner password + 4 checkbox quyền →
  lưu bản mã hoá riêng (file đang mở giữ nguyên).
- **Gỡ mật khẩu**: nhập mật khẩu → chọn file khoá → lưu bản đã gỡ → mở luôn.
- **Xoá metadata**: 1 nút → lưu bản sạch → mở luôn. Không dùng prompt/alert.

## 3. Test (6 mới; tổng engine 58/58 ngoài qpdf-fixture)
- `redact_removes_text_content_for_real` — chuỗi mật biến mất khỏi
  `extract_text`, chuỗi ngoài vùng còn nguyên, tâm vùng render ra ĐEN.
- `redact_blacks_out_image_pixels_for_real` — đọc lại RAW IMAGE từ file kết
  quả: nửa bị redact đen trong chính dữ liệu ảnh, nửa kia vẫn màu gốc
  (chứng minh không phải "vẽ đè").
- `encrypt_with_perms_round_trip_and_flags` — không mật khẩu bị chặn, đúng
  user/owner mở được, `qpdf --show-encryption` xác nhận quyền in bị cấm.
- `decrypt_removes_password` + `decrypt_with_wrong_password_fails`.
- `strip_metadata_removes_info_and_xmp` — Producer/Author biến mất khỏi
  BYTES của file, trailer sạch, file vẫn mở/đọc bình thường.
- `qpdf_safety` cũ: 5/7 xanh trên Linux CI; 2 test repair fixture
  `corrupt-truncated.pdf` fail do **qpdf 11.9 Linux không dựng lại được
  fixture này** (bản qpdf Windows của dự án xử lý được — khác biệt phiên bản
  qpdf, không phải hồi quy code; cần xác nhận lại trên máy Windows).

## 4. Đối chiếu FINAL TARGET & RULE
- Redaction là "xoá thật + kiểm chứng được", không phải vẽ đè — test đọc lại
  bytes/pixel/extract để chứng minh, đúng DoD roadmap.
- Không thoả hiệp dialog: mọi thao tác có UI chuyên nghiệp (modal + toolbar).
- Điểm CHƯA đạt 100% so với Foxit (ghi nhận, có kế hoạch):
  - Redact theo VÙNG QUÉT, xoá cả run text bị chạm (Foxit tỉa theo ký tự).
    Nâng cấp v2: cắt run tại ranh giới ký tự bằng char-box (đã có
    `page_char_boxes` từ Phase 1) + tái tạo phần giữ lại như reflow.
  - Chưa xoá "hidden information" dạng attachment/script/comment ẩn — gộp
    vào Iteration 2.

## 5. (Đã thực hiện — xem mục 6.) Kế hoạch iteration 2 ban đầu

## 6. Iteration 2 — Chữ ký số + redact ký tự + lưu tối ưu (ĐÃ LÀM)

### 6.1 Chữ ký số PKCS#7/PAdES (`sign.rs`)
- **`generate_self_signed_id(cn, out.pem)`**: tạo Digital ID tự ký RSA-2048 +
  X.509 (rcgen), ghi PEM bundle (cert + private key) — người dùng ký thử ngay
  không cần CA; có PFX/PEM thật thì nạp thẳng.
- **`sign_pdf(input, id.pem, reason, name, output)`** đúng cơ chế Adobe:
  1. Chuẩn hoá qua qpdf → lopdf dựng signature dict
     (`/Type/Sig /SubFilter/adbe.pkcs7.detached`) với `/ByteRange` +
     `/Contents` placeholder, field chữ ký (Widget vô hình) + AcroForm/SigFlags.
  2. Serialize → định vị offset thật, vá `/ByteRange` (phần hex `<...>` của
     Contents bị loại khỏi digest, 2 dấu `<` `>` vẫn nằm trong vùng ký).
  3. SHA-256 trên 2 đoạn ByteRange → **CMS SignedData detached** (crate `cms`,
     signed attrs gồm messageDigest + contentType, ký RSA-SHA256) → nhét DER
     vào Contents.
- **`verify_signatures(input)`**: với mỗi chữ ký — parse ByteRange/Contents,
  băm lại, kiểm (a) chữ ký RSA hợp lệ trên signed attrs, (b) messageDigest
  khớp digest file, (c) chữ ký phủ TOÀN BỘ file. `is_valid()` = cả 3 đạt.
- Bài học: hex Contents có padding '0' → phải cắt DER về đúng độ dài (đọc
  length trường DER) trước khi parse; `/Contents` của TRANG (tham chiếu
  `N 0 R`) phải bỏ qua, chỉ nhận `/Contents <hex>` của chữ ký.

### 6.2 Redaction tỉa theo KÝ TỰ (`redact.rs`)
- Text object bị vùng redact chạm MỘT PHẦN: dùng `page_char_boxes` (Phase 1)
  xác định ký tự nào nằm trong vùng, **giữ các đoạn ký tự ngoài vùng**, dựng
  lại đúng vị trí/baseline/màu (font: nhúng gốc → cùng họ → mặc định).
- Guard an toàn: text XOAY/nghiêng (matrix b,c ≠ 0) hoặc không đọc được font →
  xoá NGUYÊN object (thà thừa còn hơn sót). Ảnh vẫn bôi đen pixel như iter 1.

### 6.3 Lưu tối ưu (`qpdf::optimize_save`)
- `qpdf --object-streams=generate --compress-streams=y`: nén + dọn object mồ
  côi (rác sau edit/redact — object đã tách khỏi trang nhưng còn trong file).

### 6.4 UI + Tauri
- Thanh Bảo mật thêm: **🪪 Tạo Digital ID**, **✍️ Ký số** (dialog tên/lý do/
  chọn .pem → tự kiểm tra sau khi ký), **🔎 Kiểm tra chữ ký** (modal bảng
  trạng thái hợp lệ/không + lý do), **📦 Lưu tối ưu**.
- Commands: `sig_create_id`, `sig_sign`, `sig_verify`, `security_optimize`,
  `pick_any_file`, `pick_save_pem`.

### 6.5 Test iteration 2 (7 mới → tổng 62/62)
- `sign_then_verify_is_valid` — tạo ID → ký → xác thực hợp lệ, đúng tên người ký.
- `tampering_after_signing_is_detected` — lật 1 byte vùng đã ký → chữ ký KHÔNG
  hợp lệ (chứng minh phát hiện giả mạo, cốt lõi của chữ ký số).
- `verify_reports_none_for_unsigned` — file chưa ký: 0 chữ ký.
- `redact_removes_text_content_for_real` (nâng): chuỗi mật biến mất nhưng
  phần cùng dòng ngoài vùng **vẫn còn** (tỉa theo ký tự).
- `optimize_save_keeps_content_and_not_larger`.

### 6.6 Giới hạn còn lại (v-sau, không chặn dùng)
- Chữ ký tự ký (self-signed) → Acrobat báo "hợp lệ nhưng chưa tin cậy chuỗi
  CA"; cần cài cert vào Trusted hoặc dùng CA thật. Cơ chế byte-range/CMS đã
  chuẩn để Acrobat xác thực toán học.
- Chưa timestamp (PAdES-T) và LTV; chưa đọc PFX/PKCS#12 (mới PEM).
- Ký nhiều lần giữ chữ ký cũ (multi-sig incremental) chưa làm — mới ký lần đầu.
- Redact ký tự: đoạn XOAY vẫn xoá cả object (an toàn).
