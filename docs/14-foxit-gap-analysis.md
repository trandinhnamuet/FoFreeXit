# 14 — Phân tích khoảng cách so với Foxit: định hướng, công nghệ, tiến độ

> Trả lời câu hỏi chiến lược: *"Với tham vọng thay thế Foxit, đạt chất lượng
> tương đương hoặc hơn về trải nghiệm và tính năng, thì định hướng, công nghệ
> và tiến độ hiện tại có đáp ứng được không? Có vấn đề gì khiến các tính năng
> không đáp ứng được như Foxit không?"*
>
> Cập nhật lần đầu: sau Phase 4 Iteration 2 (giữ font khi sửa text).

## 1. Kết luận ngắn

**Định hướng và công nghệ: ĐÁP ỨNG ĐƯỢC, không có rào cản chết người.**
Stack Rust + Tauri + PDFium + QPDF đủ trần năng lực để đạt (và ở vài điểm vượt)
Foxit cho toàn bộ lộ trình Phase 1–8. Lý do tin được điều đó:

- **PDFium là engine của Chrome** — render/parse text/font ngang hàng engine
  thương mại; Foxit bán chính engine của họ cho Google làm gốc PDFium. Ta đứng
  trên cùng một nền engine với đối thủ.
- Những chỗ PDFium *không* expose đủ (ví dụ annotation appearance, /W width
  của font Type0, coverage cmap khi sửa text) đều đã xử lý được bằng cách
  **tự thao tác tầng PDF object qua `lopdf`/`ttf-parser`** (annot.rs,
  fontmatch.rs) — đúng thứ tự leo thang của FINAL TARGET & RULE, và đã chứng
  minh trong code chứ không phải giả định.
- Các bài toán "khó nhất" của một PDF editor (sửa text giữ font, tổ chức trang
  an toàn, file hỏng/mã hoá) đã có lời giải chạy được + test.

**Tiến độ: đúng hướng nhưng mới đi ~40% quãng đường tính năng.** Phase 1–4
(viewer, annotate, organize, edit lõi) là "core thay Foxit cho cá nhân" và đã
xong ở mức lõi. Phần "đáng tiền doanh nghiệp" (bảo mật/chữ ký số, form, OCR,
convert) chưa bắt đầu — đó là các phase 5–7 theo roadmap, không phải rủi ro
công nghệ mà là khối lượng việc.

## 2. Những vấn đề TỪNG khiến tính năng không đạt chuẩn Foxit — và cách đã giải

| Vấn đề | Hậu quả với người dùng | Giải pháp đã áp dụng (đã có test) |
|---|---|---|
| PDFium `set_text` re-encode theo font subset → mất glyph ngoài subset | Sửa text là **đổi font toàn bộ run** (Times → Arial), lệch hẳn tài liệu; chỉ đổi cỡ/màu cũng đổi font; mất đậm/nghiêng | **Iteration 2**: quyết định 3 tầng — (1) sửa TẠI CHỖ giữ nguyên font khi chắc chắn an toàn (charset-subset / base-14 + ASCII / cmap của font bytes phủ đủ); (2) thiếu glyph thật → thay font **cùng họ** metric-compatible (bảng family + fc-match + alias base-14); (3) mới đến font mặc định. Đổi cỡ = scale matrix, không đụng font |
| Cỡ chữ hiển thị vs Tf size lẫn lộn khi matrix có scale | Đặt cỡ 20 trên text scale ×2 → chữ 40pt (phóng đại kép) | Quy ước cỡ = "hiển thị"; in-place scale matrix neo baseline; tạo lại thì quy đổi ngược Tf trước khi áp matrix. Test hồi quy riêng |
| PDF cắt 1 dòng nhìn thấy thành nhiều text run | Double-click chỉ sửa được 1 mảnh chữ — trải nghiệm vụn | UI gom run cùng baseline liền kề → sửa **cả dòng**; commit 1 batch (SetText + Delete) = 1 nấc undo |
| Ô sửa inline dùng font mặc định của WebView | Đang gõ không giống kết quả (không WYSIWYG) | Ô sửa nhận đúng family/cỡ/màu/kiểu của run (CSS xấp xỉ theo `font_family` engine trả về) |
| Kéo-thả không có phản hồi trực tiếp | Cảm giác "lag", không giống Foxit | Khung đi theo con trỏ ngay khi kéo; thả mới commit |
| Annotation FreeText hiển thị sai width tiếng Việt ở viewer khác | File ghi ra "xấu" ở Acrobat | Tự dựng /W (widths) cho font Type0 nhúng bằng `ttf-parser` (Phase 2) |
| PDFium segfault khi Drop object đã remove | Crash khi xoá object | `std::mem::forget` object đã tách (đã ghi memory + comment) |
| File hỏng/mã hoá làm hỏng luồng lưu | Mất file người dùng | QPDF repair + mã hoá lại an toàn (Phase 3), test riêng |

**Bài học chung:** chưa gặp vấn đề nào *không giải được* trên stack này. Mọi
"thư viện không hỗ trợ" đến nay đều xử lý được bằng đọc source thư viện + thao
tác tầng PDF object trực tiếp.

## 3. Khoảng cách CÒN LẠI so với Foxit + giải pháp cụ thể

Xếp theo mức độ ảnh hưởng trải nghiệm:

### 3.1 Reflow đoạn văn nhiều dòng ("sửa như Word") — ✅ ĐÃ GIẢI (Iteration 3)
- **Đã làm đúng kế hoạch:** UI gom block theo baseline cách đều + giao ngang;
  engine `EditOp::ReflowText` tự suy hình học từ matrix/bounds các run, đo
  width bằng `hmtx` (ttf-parser), bẻ dòng greedy (`\n` = ngắt cứng), tạo lại
  các dòng đúng baseline spacing; font giữ theo thang 4 mức (bytes nhúng →
  font chuẩn base-14 qua `FPDFText_LoadStandardFont` → cùng họ → fallback).
  3 test integration + 3 unit — xem `docs/12` mục Iteration 3.
- **Còn lại của mục này (v2):** đoạn justify reflow về căn trái; chưa kerning;
  khối text xoay chưa reflow theo hướng xoay; đoạn lẫn nhiều font thống nhất
  theo run neo. Đây là các case Foxit xử lý tốt hơn — nâng khi gặp tài liệu
  thực tế, không chặn trải nghiệm chính.

### 3.2 Hiệu năng phiên sửa trên file rất lớn
- **Hiện tại:** mô hình "materialize tức thì" — mỗi thao tác lưu toàn bộ tài
  liệu ra file tạm mới rồi render lại. Đúng tuyệt đối (WYSIWYG thật, undo chắc
  chắn), nhưng file vài trăm MB sẽ trễ nhìn thấy được ở mỗi thao tác; Foxit
  chỉnh trong RAM và lưu 1 lần.
- **Giải pháp theo nấc (không phá kiến trúc):**
  1. (rẻ) Giữ document PDFium mở suốt phiên sửa trong Tauri state, áp op
     in-memory, chỉ render lại trang — bỏ chu kỳ load-từ-đĩa mỗi op; save ra
     temp chỉ để làm mốc undo (có thể async).
  2. (vừa) Undo bằng danh sách op thay vì snapshot file: replay từ gốc khi
     undo (op rẻ vì in-memory).
  3. (nếu cần) Incremental save của QPDF cho file khổng lồ.
- **Ghi chú:** với file văn phòng thông thường (<20MB) mô hình hiện tại đã đủ
  mượt; nấc 1 chỉ là việc tổ chức lại Tauri command, không đổi engine.

### 3.3 Lưu file sau sửa: font nhúng full + object rác
- **Hiện tại:** khi phải thay font (tầng 2/3), font mới nhúng **nguyên cả file
  TTF** (Arial ~1MB); object bị thay vẫn nằm lại file dưới dạng rác (đã tách
  khỏi trang nên không hiển thị, nhưng tốn dung lượng). Foxit subset font khi
  lưu và dọn object rác.
- **Giải pháp:** bước "Lưu tối ưu" chạy khi save: (1) subset font nhúng theo
  đúng glyph dùng thật — tự viết được trên `ttf-parser` (đã dựng /W thủ công ở
  annot.rs, subset TTF là bài quen thuộc: lọc glyf/loca/cmap/hmtx); (2) chạy
  `qpdf --object-streams=generate` (đã có sẵn qpdf.rs) để dọn rác + nén object
  stream. Đưa vào Phase 5 cùng nhóm "lưu file vững".

### 3.4 Độ phủ font hệ thống khi thay thế
- **Hiện tại:** bảng family phổ biến (Windows ~16 họ, macOS, Linux
  Liberation + fc-match). Font ngoài bảng → fallback mặc định.
- **Giải pháp:** Windows dùng DirectWrite/GDI enumeration (crate `font-kit`
  hoặc đọc registry Fonts) để match theo TÊN THẬT của mọi font đã cài thay vì
  bảng tĩnh; giữ bảng tĩnh làm fallback offline. Việc nhỏ, độc lập, làm khi
  chạm phải file thực tế đầu tiên không match.

### 3.5 Các tính năng Foxit chưa bắt đầu (không phải rào cản kỹ thuật)
| Nhóm | Đường đi đã chốt trong roadmap | Rủi ro công nghệ |
|---|---|---|
| ~~Bảo mật, redaction, chữ ký số PAdES (Phase 5)~~ ✅ ĐÃ LÀM | AES-256+quyền, redaction thật (tỉa ký tự), **chữ ký số PKCS#7/CMS + xác thực + phát hiện giả mạo** (`sign.rs`), lưu tối ưu — 62/62 test | Đã giải; còn lại timestamp/LTV/PFX/multi-sig (docs/15 mục 6.6) |
| ~~Form AcroForm (Phase 6)~~ ✅ ĐÃ LÀM | Liệt kê/điền/tạo field (lopdf) + flatten (PDFium) + FDF/CSV — 67/67 test (`form.rs`) | Đã giải; còn XFDF, radio-group/list/pushbutton khi tạo (docs/17 mục 6) |
| ~~OCR (Phase 7)~~ ✅ ĐÃ LÀM | Tesseract sidecar → lớp text ẩn khớp toạ độ, Việt+Anh có test đúng dấu (`ocr.rs`) | Đã giải; còn tiền xử lý deskew/denoise, OCR theo vùng |
| ~~Convert Office↔PDF (Phase 7)~~ ✅ ĐÃ LÀM | LibreOffice headless 2 chiều + PDF→DOCX/TXT/PNG tự viết fallback (`convert.rs`) | Đã giải mức "đủ dùng"; PDF→Excel + layout hoàn hảo là follow-up |
| So sánh PDF, in ấn, installer (Phase 8) | render diff đã có nền; Tauri bundler | Thấp |

## 4. Đánh giá định hướng công nghệ (giữ hay đổi?)

**GIỮ.** Đánh giá lại từng lựa chọn sau 4 phase:

- **PDFium qua `pdfium-render`**: đúng đắn. Đã 2 lần tưởng "không làm được"
  (annotation appearance, giữ font khi sửa) và cả 2 lần đều giải được bằng
  API sẵn có + tự bù tầng PDF object. Trần năng lực còn xa.
  *Lưu ý vận hành:* `pdfium-render` 0.8.37 có API deprecated đổi nghĩa
  (`set_matrix` = apply, không phải replace) — đã ghi chú trong code; khi nâng
  version cần đọc CHANGELOG cẩn thận.
- **Tauri/WebView UI**: đủ mượt cho viewer (lazy render 1000+ trang đã chứng
  minh ở Phase 1). Điểm cần canh: thao tác chuột mô phỏng trong WebView2 khó
  test tự động → bù bằng checklist test tay (docs/13) — chấp nhận được.
- **QPDF sidecar**: gọn, đúng việc (repair/mã hoá/nén), không cần thay.
- **Rust workspace tách engine/UI**: cho phép test engine không cần UI — chính
  là lý do 50 test tự động chạy được trong CI/Linux dù app nhắm Windows.

**Điều KHÔNG nên làm:** tự viết engine PDF từ đầu, hay chuyển C++/Qt để "giống
Foxit hơn" — chi phí khổng lồ, không giải quyết gap nào ở mục 3.

## 5. Tiến độ so với đích + thứ tự việc đề xuất

Đã xong (lõi, có test): Viewer ✅ · Annotate ✅ · Organize + lưu vững ✅ ·
**Edit text/object giữ font ✅ (iteration 2) · Reflow đoạn "như Word" ✅
(iteration 3) · Bảo mật iteration 1 ✅ (mã hoá AES-256 + quyền, gỡ mật khẩu,
redaction thật kiểm chứng được, xoá metadata — docs/15)**.

**Phase 5 đã HOÀN TẤT** (iter 1+2): mã hoá+quyền, gỡ mật khẩu, redaction thật
+ tỉa ký tự, xoá metadata, **chữ ký số PKCS#7/PAdES + xác thực**, lưu tối ưu.

**Phase 6 + 7 đã HOÀN TẤT lõi**: form đầy đủ; OCR Việt+Anh; convert
PNG/TXT/DOCX + Office↔PDF.

Thứ tự đề xuất tiếp theo, bám giá trị người dùng:
1. **Phase 8 — Hoàn thiện & Phát hành**: installer Windows (MSI/winget qua
   Tauri bundler), in ấn, so sánh 2 PDF, preferences, đa ngôn ngữ UI (Việt/
   Anh), tối ưu hiệu năng + bộ test hồi quy đầy đủ.
2. Song song, rải dần: mở rộng font matching (3.4), nấc 1 hiệu năng phiên
   sửa (3.2), reflow v2 (justify/xoay — 3.1), nâng chữ ký (timestamp/LTV/
   PFX/multi-sig), nâng form (XFDF, radio-group), OCR tiền xử lý ảnh,
   PDF→Excel.

## 6. Checklist FINAL TARGET & RULE cho chính tài liệu này
- [x] Đối chiếu từng tính năng với hành vi Foxit làm chuẩn nghiệm thu.
- [x] Không đề xuất cắt giảm tính năng nào; mọi gap đều kèm giải pháp cụ thể.
- [x] Giải pháp theo đúng thứ tự: dùng API thư viện → bù tầng PDF object → tự viết.
- [x] Mỗi gap có tiêu chí nghiệm thu so với Foxit (mục 3).
