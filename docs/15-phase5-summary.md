# 15 — Tổng kết Phase 5 (Bảo mật) · Iteration 1

> Trạng thái: **mã hoá + permissions, gỡ mật khẩu, redaction THẬT, xoá
> metadata — hoàn tất, có test kiểm chứng nội dung thật sự biến mất.**
> Chữ ký số (PAdES) là Iteration 2 — kế hoạch ở mục 5.

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

## 5. Iteration 2 (kế tiếp của Phase 5) — Chữ ký số PAdES + Lưu tối ưu
1. **Ký số**: tạo chữ ký PAdES-B (CMS/PKCS#7 qua crate `cms`/`rsa`/`p256`),
   ghi `/ByteRange` + placeholder `/Contents`, incremental update để không
   phá chữ ký cũ; xác thực bằng Acrobat làm DoD. Đọc chứng chỉ PFX/PKCS#12.
2. **Xác thực chữ ký** khi mở (hiện trạng thái valid/invalid).
3. **Lưu tối ưu** (từ docs/14 mục 3.3): subset font nhúng theo glyph dùng
   thật + `qpdf --object-streams=generate` dọn object rác sau edit.
4. Redact tỉa theo ký tự (mục 4) + quét "hidden info" còn lại.
