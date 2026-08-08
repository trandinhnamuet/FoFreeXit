# 12 — Tổng kết Phase 4 (Chỉnh sửa nội dung — Edit) · Iteration 1 + 2 + 3 + 4

> Trạng thái sau Iteration 4: sửa được cả text **NẰM TRONG Form XObject**
> (file Canva/Illustrator/InDesign gói cả trang vào form).

## Iteration 4 — Sửa text trong Form XObject (file Canva...)

User mở CV xuất từ Canva: xem được nhưng **không sửa được gì** — trang chỉ có
10 path + 1 Form XObject phủ toàn trang, TOÀN BỘ chữ nằm trong form (16 font
Type0/CID nhúng), trong khi `list_objects` chỉ duyệt object cấp trang.

### Engine (`edit.rs`)
- **Danh sách PHẲNG đệ quy** (`collect_flat`): duyệt object trang theo thứ tự
  vẽ, đi sâu vào Form XObject qua `FPDFFormObj_CountObjects/GetObject`
  (pdfium-render 0.8.37: `PdfPageXObjectFormObject::len()/get()` public; match
  thẳng variant `PdfPageObject::XObjectForm` để giữ lifetime trang — accessor
  `as_x_object_form_object()` bị rút ngắn lifetime theo `&self`). Mỗi mục có
  `path` ([i] cấp trang, [i,j,…] con trong form) + `acc` = tích ma trận form
  tổ tiên. Chặn depth 3 / 4000 mục. **`index` của ObjectInfo/EditOp = vị trí
  trong danh sách phẳng này** — trang không có form thì trùng index trang như
  cũ (tương thích ngược 100%, UI không đổi cách gửi op).
- **Toạ độ/cỡ quy về trang**: rect con = AABB của 4 góc qua `acc`; cỡ hiển
  thị nhân `mat_vscale(acc)` (độ dài ảnh vector đơn vị Y).
- **ObjectInfo thêm `nested`** (con trong form — sửa text OK, kéo/resize chưa)
  **+ `expanded`** (form đã liệt kê con — UI bỏ khung form để khỏi che con).
- **KẾT LUẬN QUAN TRỌNG NHẤT (3 vòng CI + đọc source PDFium): PDFium hiện
  KHÔNG ghi lại được stream của Form XObject.** Chuỗi chứng cứ:
  1. `FPDFText_SetText` trên con không làm form dirty (`dirty_streams_` chỉ
     được đánh dấu bởi Remove/Insert trên chính form — cpdf_pageobjectholder.cpp);
  2. rút con bằng `FPDFFormObj_RemoveObject` + làm trang dirty → `ProcessForm`
     CÓ chạy generator cho form, NHƯNG `CPDF_PageContentManager` (nơi ghi kết
     quả) chỉ biết key `/Contents` — form là stream TỰ THÂN, không có
     /Contents → content mới rơi vào key /Contents vô nghĩa trên dict form,
     stream thật giữ nguyên (cpdf_pagecontentmanager.cpp, constructor +
     AddStream). Test round-trip bắt đúng triệu chứng: extract sau lưu vẫn
     thấy text cũ.
- **Giải pháp cuối: MỞ GÓI (flatten) form ra cấp trang** —
  `flatten_form_xobjects`: rút từng con khỏi form (`FPDFFormObj_RemoveObject`
  — cần đổi feature pdfium-render `pdfium_latest`→`pdfium_7350` vì wrapper
  0.8.37 sót gate; đã quét 0 hàm 7543-only bị mất), nhân ma trận form vào
  con, chèn lại vào TRANG, xoá form rỗng; lặp ≤4 vòng cho form lồng form.
  Hiển thị y hệt (đã kiểm vị trí/nội dung), mọi object thành cấp trang → sửa
  bằng đường page-level đã được test kỹ từ iteration 1-3. UI: `loadEditPage`
  thấy object `nested` → gọi `edit_flatten_to_temp` một lần rồi nạp lại
  (bản mở gói không tính là "có thay đổi" cho tới khi sửa thật).
- Op nhắm vào object trong form khi CHƯA flatten → engine trả lỗi rõ ràng
  ("trang chưa được mở gói") thay vì âm thầm ghi bản không đổi — có test.
- CI test chạy `--test-threads=1`: PDFium serialize qua 1 mutex toàn cục —
  1 test panic (kể cả panic Ở ASSERT khi instance Pdfium còn sống — unwind
  drop guard giữa panic) sẽ poison mutex, mọi test SAU đó trong cùng process
  chết oan tại thread_safe.rs:76. Khi đọc log CI: chỉ failure ĐẦU TIÊN là
  lỗi thật.
- Vá kèm: đọc source crate pdfium-render tải từ crates.io để chốt API vì máy
  dev không compile được Rust (xem mục CI bên dưới).

### UI (`main.js`)
- Bỏ khung overlay cho form `expanded` (khung phủ kín trang chặn hết click)
  và cho ảnh/form `nested`; run text trong form tự chảy qua clusterTextLines
  như run thường (rect đã quy về trang).
- Chặn kéo di chuyển dòng chứa run `nested` (engine chưa hỗ trợ Transform
  trong form) — vẫn chọn/sửa text/xoá bình thường.

### Test (6 — `edit_roundtrip.rs`, fixture tự dựng bằng lopdf)
- Fixture: PDF 1 trang, chữ "Hello inside form" Helvetica 24pt nằm TRONG
  Form XObject (trang chỉ có 1 object form) — đúng cấu trúc Canva.
- `form_xobject_children_are_listed` — thấy text nested, form expanded, rect
  quy về trang đúng chỗ (72, ~700), cỡ ~24.
- `flatten_form_xobjects_unwraps_page` — mở gói: hết form, text thành cấp
  trang, vị trí giữ nguyên, nội dung không đổi.
- `form_xobject_settext_after_flatten_roundtrip` / `..._reflow_...` /
  `..._delete_...` — sửa/reflow tiếng Việt/xoá SAU flatten round-trip chuẩn.
- `nested_target_without_flatten_is_rejected` — op vào object trong form khi
  chưa flatten bị từ chối với thông điệp rõ.

### CI chạy test engine
Máy dev không có Rust nên **CI là nơi duy nhất chạy test**: workflow thêm
bước `cargo test -p ff-engine` TRƯỚC khi build release (fail test = fail
build, không publish bản hỏng); cache thêm `target/` gốc workspace.

### CỔNG AN TOÀN + giới hạn ghi nhận (iteration 4 v1)
Kiểm chứng E2E trên file Canva THẬT (CV, 1149 object sau mở gói) phát hiện:
**generator của PDFium ghi lại cả trang là LOSSY với file phức tạp**:
- Map font theo (type, BaseFont) — Canva nhúng 2+ subset TRÙNG TÊN
  ("AAAAAA+Now-Bold" ×2): run của subset 2 bị ép qua subset 1 → mất glyph
  rải rác (PROFILE → "O ILE").
- `ProcessImage` BỎ ảnh inline (`IsInline() → return`) — mất ảnh chân dung.
- Mở form theo vòng append cuối trang → z-order xáo trộn (panel đè lên chữ).

→ **`edit_flatten_to_temp` có CỔNG AN TOÀN `page_render_mismatch`**: render
trang trước/sau mở gói (500px, dung sai 12/255/kênh), lệch >0.5% điểm ảnh →
HUỶ file tạm + trả lỗi rõ; UI hiển thị thông điệp và CHẶN mở ô sửa phần nằm
trong form (xem/chú thích/tổ chức trang vẫn bình thường). File gói form đơn
giản (đa số file xuất máy in ảo, fixture test) qua cổng → sửa thoải mái.

**Iteration 4b — PHẪU THUẬT stream (ĐÃ LÀM, thay thế flatten ở UI)**:
`formsurgery.rs` (lopdf): xoá đúng op vẽ chữ (Tj/TJ/'/") của run bị sửa
trong stream form — map text-child thứ k ↔ show-op thứ k, form con thứ n ↔
Do-trỏ-form thứ n (thứ tự parse PDFium tuần tự); MỖI MỨC có bất biến kiểm
đếm (số op == số object PDFium thấy — lệch là từ chối); '/" thay bằng
T*/Tw+Tc+T* giữ hiệu ứng vị trí. Chữ mới vẽ ở CẤP TRANG (regenerate trang
không đụng stream form → không dính lỗi trùng tên font). `apply_edits`:
run nested trong ReflowText/Delete → phẫu thuật ở pha (A2) rồi MỞ LẠI
document (tầng-0 token gốc tắt cho anchor nested — token vô hiệu sau mở
lại); cổng an toàn `page_render_mismatch_masked`: pixel ngoài vùng khối sửa
lệch >0.5% → huỷ output. UI bỏ auto-flatten, sửa nested trong suốt qua đúp
chuột; SetText thuộc tính/kéo khối trong form chặn-với-thông-điệp.
Bẫy lopdf: `decompressed_content()` BÁO LỖI với stream không nén (thiếu
/Filter) → fallback bytes thô.
**Kiểm chứng E2E trên CV Canva thật (build 26)**: sửa 1 dòng trong form
(1155 object nested) → commit thành công, chữ mới đúng chỗ/đúng màu, ảnh
chân dung + toàn bộ chữ khác nguyên vẹn (PROFILE/CONTACT đủ glyph), cổng
masked pass, undo hoạt động. 2 test round-trip surgical (delete/reflow giữ
nguyên phần khác của form) + test SetText nested bị từ chối.

Giới hạn khác:
- Flatten làm mất clip/ExtGState/transparency group đặt Ở MỨC FORM — các
  trường hợp này thường cũng làm lệch render → cổng an toàn tự chặn.
- Cấu trúc file sau khi SỬA thay đổi (form mở ra trang) — hiển thị giữ
  nguyên (đã có cổng kiểm); chỉ xảy ra khi người dùng thật sự lưu.
- Form lồng sâu >4 vòng mở hoặc >4000 object: phần vượt giữ nguyên trong form.

> Các iteration trước: 52/52 test engine xanh ngoài qpdf (17 test edit + 6
> unit fontmatch + 29 test cũ) — nay thêm 4 test form + 2 unit sfnt style.

## Iteration 3 — Reflow đoạn văn "như Word" (mới nhất)

Đóng gap trải nghiệm cuối cùng của tính năng moat (mục 3.1 của
`docs/14-foxit-gap-analysis.md`): double-click vào đoạn nhiều dòng → sửa cả
đoạn trong 1 ô, chữ tự bẻ dòng lại theo bề rộng khối khi commit.

### Engine — `EditOp::ReflowText { indices, text }`
- Engine TỰ suy hình học từ các run: lề trái/bề rộng khối từ bounds, baseline
  từ `matrix.f` (gom cụm ±1pt), khoảng cách dòng = median hiệu baseline
  (1 dòng → 1.25× cỡ hiển thị) — UI chỉ cần gửi indices + text mới.
- **Bẻ dòng đo bằng hmtx thật** (`fontmatch::wrap_lines` + `char_advance` qua
  ttf-parser): greedy theo từ, `\n` = ngắt cứng, từ quá dài cắt theo ký tự,
  dòng rỗng giữ nhịp baseline. Không kerning (chấp nhận v1, nới 2%).
- **Giữ font theo thang 4 mức** (nhất quán triết lý iteration 2):
  1. Font NHÚNG parse được + phủ đủ glyph → nhúng lại chính bytes đó (glyph y hệt);
  2. Family nhóm base-14 (Helvetica/Times/Courier + Arial alias) + text ASCII →
     **font chuẩn PDF qua `FPDFText_LoadStandardFont`** — BaseFont giữ tên chuẩn,
     KHÔNG nhúng, file không phình; đo width bằng font metric-compatible
     (Liberation/Arial);
  3. Font hệ thống cùng họ (coverage-checked); 4. fallback mặc định.
- Dòng mới giữ phần tuyến tính matrix của run neo (scale/nghiêng), gốc tại
  (lề trái, baseline thứ i); Tf = unscaled của run neo → cỡ hiển thị không đổi.
- Đoạn dài ra thì các dòng mới nối xuống dưới theo đúng nhịp baseline (hành vi
  tương tự Foxit khi khối text nở ra).

### UI
- Double-click: **đoạn ≥2 dòng → textarea sửa cả đoạn** (`paragraphLines` gom
  dòng baseline cách đều ±25%, cỡ chữ tương đồng, giao ngang ≥30%); 1 dòng →
  ô sửa dòng như iteration 2.
- Textarea WYSIWYG: đúng font/cỡ/màu/kiểu + line-height đúng nhịp baseline;
  nội dung khởi tạo = các dòng nối bằng khoảng trắng (đoạn chảy tự nhiên);
  **Enter = ngắt cứng, Ctrl+Enter hoặc bấm ra ngoài = áp dụng, Esc = huỷ**.
- 1 commit = 1 op `reflowText` = 1 nấc undo.

### Test mới (3 integration + 3 unit)
- `reflow_wraps_and_keeps_embedded_font` — đoạn 3 dòng font nhúng + text Việt
  dài gấp mấy lần → bẻ >3 dòng, mọi dòng trong bề rộng khối, **font nhúng giữ
  nguyên**, baseline đều 15pt±2, text cũ biến mất.
- `reflow_hard_break_creates_new_line` — `\n` ra đúng 2 dòng cách 1 nhịp.
- `reflow_base14_ascii_keeps_standard_font` — Helvetica không nhúng + ASCII
  dài → nhiều dòng, **BaseFont vẫn "Helvetica" chuẩn** (builtin, không nhúng).
- Unit `wrap_lines`: greedy theo từ / ngắt cứng + dòng rỗng / cắt từ quá dài.

### Vá theo phản hồi file thật (Word-export, run cắt theo từ/ký tự)
Kiểm với PDF thật xuất từ Word (tiêu đề 2 dòng bị cắt thành 41 run per-word/
per-char xen run rỗng, cỡ 22.5pt + 20pt, bbox chữ có dấu tụt thấp):
- **UI cluster DÒNG theo GIAO DỌC** (≥50% chiều cao nhỏ) thay vì so baseline —
  miễn nhiễm dấu tiếng Việt; overlay 1 khung/dòng (file thật: 231 ô → 18 khung);
  chọn/kéo/xoá/đổi thuộc tính tác động CẢ DÒNG (batch ops).
- **Đúp mở CẢ ĐOẠN giữ xuống dòng** (`\n` giữa các dòng gốc — sửa được cả vị
  trí ngắt dòng); đúp trúng KHE giữa các từ vẫn mở (hit-test theo điểm);
  click trong ô sửa di chuyển con trỏ, không thoát edit; ô che kín chữ cũ.
- **Engine ReflowText tự NỞ danh sách run theo bbox khối** (tâm nằm trong
  union + 2pt) — không bao giờ sót run rỗng/lệch bbox → hết cảnh chữ mới đè
  chữ cũ; **phát hiện khối căn giữa** (tâm các dòng trùng tâm khối) → dòng mới
  giữ căn giữa. 2 test hồi quy mới (nở indices, giữ căn giữa) — 19 test edit.

### Vá vòng 2 theo phản hồi file thật (□ thay dấu cách, vỡ dòng, ô sửa lệch)
Ba lỗi user báo sau khi thử bản build 6 trên chính file Word-export:
- **Dấu cách thành ô vuông □ sau khi lưu**: PDFium `FPDFText_LoadFont` nạp
  LẠI font subset (Word xuất) rồi `SetText` sẽ map U+0020 ra `.notdef` (glyph
  hộp) dù cmap của font CÓ space — lỗi nằm ở PDFium, không phải font. Fix:
  **tầng 0 mới của thang font reflow — dùng LẠI chính font object gốc trong
  PDF** (lấy `PdfFontToken` từ run neo, không nạp lại): glyph + charcode map
  gốc xử lý dấu cách chuẩn, file không phình thêm bản font nữa. Chỉ chọn sau
  khi **probe ghi/đọc-lại toàn bộ text mới** thành công (tạo object nháp →
  so text → gỡ). Khi ghi từng dòng cũng **đọc lại ngay để tự kiểm** — nếu
  PDFium vẫn âm thầm vứt ký tự thì gỡ và **ghi lại theo TỪNG TỪ tự đặt vị trí
  bằng advance hmtx** (không bao giờ ghi glyph dấu cách) — lưới an toàn cuối.
- **Thêm 1 ký tự làm vỡ dòng** ("HỢP"→"HỢPP" đẩy "LOG" rơi xuống dòng mới):
  dòng cứng (`\n` từ ô sửa) giờ được **NỞ tới +35% bề rộng khối** (chặn mép
  trang) trước khi phải bẻ lại — hành vi hộp text tự nở của Foxit; khối căn
  giữa nở đều 2 phía quanh tâm.
- **Mỗi dòng giữ đúng CỠ CHỮ GỐC của dòng đó** (tiêu đề 22.5/20pt không còn
  bị ép cả khối về cỡ run neo): kế hoạch reflow ghi `line_styles` theo từng
  baseline gốc, dòng cứng thứ i ăn cỡ dòng gốc thứ i.
- **Ô sửa WYSIWYG**: thay textarea bằng `contenteditable` — mỗi dòng gốc là
  1 div với đúng **font/cỡ/màu/đậm-nghiêng CỦA DÒNG ĐÓ**, khối căn giữa hiện
  căn giữa, khung đặt trùng bbox khối (bù lệch dọc line-box) → chữ trong ô
  nằm NGUYÊN vị trí chữ gốc khi bắt đầu sửa; con trỏ đặt đúng chỗ vừa đúp
  (`caretRangeFromPoint`).
- Kiểm chứng trên file thật: sửa "HỢP"→"HỢPP" giữ đúng 2 dòng căn giữa cỡ
  22.5/20pt, font `BAAAAA+TimesNewRomanPS-BoldMT` gốc, dấu cách nguyên vẹn,
  render so khớp bản gốc. 2 test hồi quy mới (`reflow_keeps_per_line_font_sizes`,
  `reflow_hard_line_grows_without_rewrap`) — 21 test edit.

### Vá vòng 3 — ô sửa trùng khít chữ gốc bằng ĐO–HIỆU CHỈNH (fix hồi quy vòng 2)
User báo (kèm ảnh): ô sửa hiện chữ SAI font + SAI cỡ + SAI vị trí. Ba nguyên nhân:
- **Hồi quy vòng 2**: `perLineAdvances` tính line-height mỗi dòng bằng KHOẢNG HỞ
  giữa 2 bbox (`bottom trên − top dưới` ≈ 0/âm → kẹp 8px) thay vì khoảng cách
  baseline → các dòng dồn cục. Fix: advance = hiệu mép DƯỚI 2 bbox liền kề
  (bbox PDFium ôm sát glyph nên bottom bám baseline).
- **Sai font**: engine trả family PostScript CamelCase ("TimesNewRoman",
  "SegoeUI") không khớp tên font cài trên Windows → CSS rơi về serif/sans mặc
  định. Fix: `cssFontStack` tách CamelCase thành tên có dấu cách + bảng alias
  (Helvetica→Arial, TimesNewRomanPS→Times New Roman…).
- **Sai cỡ với file lạ**: cỡ px suy mở từ `font_size` engine, không tự kiểm.
  Fix: **`fitEditLinesToPdf` — vòng đo–hiệu chỉnh sau khi mount ô sửa**:
  (1) CỠ: chiều cao MỰC của đúng chuỗi đó (canvas `measureText` actualBoundingBox,
  đo ở cỡ tham chiếu 100px để né sai số hinting ~5% ở cỡ nhỏ) phải bằng chiều
  cao bbox dòng PDF (cùng là ink-bbox, PDFium bounds = union glyph box) — engine
  báo cỡ lệch bao nhiêu cũng tự sửa về đúng, lệch <12% coi là khác metric font
  → giữ cỡ engine (ổn định file chuẩn); (2) VỊ TRÍ: quy 2 phía về MÉP MỰC TRÊN
  (DOM: Range rect + (fontAscent − inkAscent) từ canvas; PDF: rect.top) rồi bù
  `margin-top`/`text-indent` từng dòng (dòng đầu bù vào khung để nền trắng che
  kín); khối căn giữa cũng bù ngang qua text-indent. Thêm: giữ thụt lề từng
  dòng; đúp từ viewer thường nay đặt con trỏ đúng điểm đúp (toạ độ giả lập từ
  điểm PDF, chờ `img.decode()` trước khi đo).
- **Kiểm chứng**: harness HTML mô phỏng trang bằng canvas (chữ gốc đỏ, bbox mực
  thật làm ground truth) + Edge headless chụp 3 kịch bản: (A) engine báo cỡ sai
  MỘT NỬA + family CamelCase → tự sửa về đúng, lệch ≤0.4px dọc / 0px ngang;
  (B) engine báo đúng → dead-band giữ nguyên, trùng khít 100%; (C) khối căn
  giữa Times → lệch ngang ≤1.3px. (Không unit-test tự động được vì cần layout
  engine thật; harness lưu ở scratchpad phiên làm việc.)

### Vá vòng 4 — khối sửa chuẩn Foxit: tách đoạn theo FORMAT, nền trong suốt, bullet, in đậm
User test build 11 trên file thật, báo 4 lỗi (kèm ảnh). Đối chiếu hành vi
Foxit (help.foxit.com — Edit Text): mỗi ĐOẠN là 1 text block riêng, click vào
đâu chỉ sửa block đó, reflow trong block; muốn sửa nhiều block phải Link thủ
công. Sửa tương ứng:
- **Gom đoạn quá tham** (tiêu đề + dòng code + thân bài + nhãn đỏ dính 1 khối):
  `paragraphLines` giờ chỉ gom dòng kề nhau **CÙNG FORMAT** — cùng font family
  + đậm/nghiêng + màu (của run chữ đầu dòng), cỡ chênh ≤1.25× (tiêu đề 22.5/20
  vẫn 1 đoạn, tiêu đề ≠ thân bài) — cộng điều kiện hình học cũ (hở ≤1.3× cao
  dòng, giao ngang ≥30%, nhịp đều ±35%).
- **Bullet thành ô vuông □ trong ô sửa**: bullet Word/LibreOffice là run font
  Symbol/Wingdings mã PUA (U+F0B7…) DOM không render được. Thêm `isMarkerRun`
  (run chữ trái nhất của dòng, ≤2 ký tự, PUA/ký tự bullet/font symbol; "-","*",
  "o" chỉ nhận khi có khoảng hở rõ với chữ sau) + `stripLineMarkers`: marker bị
  LOẠI khỏi ô sửa/commit/rect khối (thụt lề tính từ CHỮ) — bullet giữ nguyên
  trên trang như Foxit. Đúp trúng chính bullet vẫn mở khối chữ của dòng đó.
- **Ô sửa nền trắng che nền gốc**: giờ mở ô xong render NGẦM bản trang đã ẨN
  các run đang sửa (`edit_apply_to_temp` với Delete — chỉ lấy ảnh, commit vẫn
  tính trên editBase gốc) → đổi ảnh stage + nền ô chuyển TRONG SUỐT: thấy dải
  màu nền/bullet/kẻ bảng dưới chữ đang gõ, đúng cảm giác Foxit. Chờ render thì
  tạm nền trắng; huỷ/không đổi → trả ảnh gốc; file tạm vào editTemps để dọn.
- **Mất in đậm dù gốc đậm**: file Word/Chrome xuất subset không khai
  /FontWeight và tên không chứa "Bold" → engine `text_object_style` thêm
  fallback đọc **OS/2 usWeightClass ≥600 + cờ italic từ bytes font nhúng**
  (ttf-parser); `list_objects` cache theo tên font (file 200+ run chung vài
  font, không đọc lại bytes).
- **Kiểm chứng**: test Node trích HÀM THẬT từ main.js (11 case: tách khối theo
  format/màu, tiêu đề 2 cỡ vẫn 1 đoạn, bullet loại khỏi khối + rect từ chữ,
  đúp trúng marker, phân biệt "-" gạch đầu dòng vs "-5°C") — tất cả PASS.
  Phần render nền ẩn cần app thật → kiểm trên build CI.

### Vá vòng 5 — TÌM RA GỐC RỄ: field IPC lệch tên; kiểm chứng E2E trên app thật
User test build 12 với file thật (LibreOffice export, NotoSans): vẫn mất in
đậm. Lần này debug TRÊN CHÍNH APP đang chạy (WebView2 remote debugging qua
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port` + CDP) và
phát hiện **nguyên nhân gốc của cả chuỗi lỗi font**: DTO Tauri serialize
`rename_all = "camelCase"` (`fontBold`, `fontFamily`, `fontSize`…) nhưng toàn
bộ code edit-mode trong main.js đọc snake_case (`font_bold`…) → **mọi thuộc
tính font trong UI luôn undefined từ đầu**: không đậm, sai family, cỡ mặc
định 12 (giải thích luôn ảnh lỗi "chữ nhỏ đều nhau" ban đầu — vòng 3 đã chữa
TRIỆU CHỨNG cỡ chữ bằng đo mực, vòng này chữa đúng BỆNH). Fix: đổi hết sang
camelCase (nhất quán với phần còn lại của app: `widthPt`, `pageIndex`…).
- **Engine đọc kiểu chữ từ bytes font — vá tiếp cho LibreOffice**: subset của
  LibreOffice VỨT bảng OS/2 (kiểm bằng qpdf trích FontFile2: chỉ còn cmap/
  glyf/head/hhea/…) nên ttf-parser `weight()` trả 400 mặc định. Thêm
  `fontmatch::style_from_font_bytes` parse sfnt thủ công: OS/2 usWeightClass/
  fsSelection nếu còn, fallback **`head.macStyle`** (bảng bắt buộc, bit0=bold
  bit1=italic — file thật: 0x1 cho NotoSans-Bold, 0x0 cho Regular). 2 unit
  test sfnt tự dựng. (File này BaseFont có "-Bold" nên tên đã bắt được; vá để
  chống file tên không nói gì.)
- **Kéo-thả mở file**: nghe `tauri://drag-drop` (Tauri v2, dragDropEnabled mặc
  định) → thả .pdf từ Explorer vào cửa sổ là mở (thoát edit/organize mode
  trước); thả file khác báo "Chỉ mở được file .pdf".
- **Ẩn khung dòng overlay khi ô sửa đang mở** (nền trong suốt làm khung chấm
  lộ qua gây nhiễu) — khôi phục khi đóng.
- **Kiểm chứng E2E trên app thật + file thật** (không cần rebuild): inject
  hàm đã sửa vào app build-12 đang chạy qua CDP `Runtime.evaluate` (main.js
  là classic script nên function declaration đè được binding global), gọi
  `edit_list_objects` lấy run thật, điều khiển mở ô sửa và chụp màn hình:
  tiêu đề 2 dòng bold 25.48px (=18.5pt đúng engine) 1 khối nền trong suốt;
  khối bullet 8 dòng không PUA, bullet gốc còn nguyên trên trang. Test Node
  (hàm thật + dữ liệu run thật từ engine): tiêu đề gom [27,28], khối bullet
  loại sạch OpenSymbol, nhãn đậm màu tách riêng — tất cả PASS.

### Giới hạn ghi nhận (v1, sẽ nâng ở vòng sau)
- Đoạn justify (giãn đều 2 lề) reflow về căn trái; chưa kerning; khối text
  XOAY chưa reflow theo hướng xoay (dòng mới đặt theo trục ngang).
- Khối lẫn NHIỀU font/cỡ trong 1 ĐOẠN: cỡ giữ theo TỪNG DÒNG; font/màu theo
  run neo (dòng lẫn nhiều font sẽ thống nhất font).

> Iteration 1: 43/43 test, đã chạy app thật + chụp ảnh xác minh.

## Iteration 2 — Giữ font gốc + trải nghiệm Foxit (mới)

Đóng khoảng cách lớn nhất so với Foxit được chỉ ra khi review: **mọi lần sửa
text đều bị đổi sang font hệ thống** (Helvetica → Arial/DejaVu), kể cả khi chỉ
đổi cỡ/màu; bold/italic gốc bị mất; sửa theo mảnh run thay vì cả dòng.

### Engine (`edit.rs` viết lại + `fontmatch.rs` mới)
- **`SetText` 3 tầng, ưu tiên GIỮ FONT GỐC** (sửa tại chỗ bằng `FPDFText_SetText`,
  không xoá/tạo lại):
  1. *In-place an toàn chắc chắn*: text mới chỉ dùng ký tự đã có trong run; hoặc
     font **không nhúng** (base-14 Helvetica/Times…) + text mới toàn ASCII
     (BaseFont khai báo giữ nguyên trong file — đúng hành vi Foxit); hoặc cmap
     của font (đọc qua `FPDFFont_GetFontData` + `ttf-parser`) phủ đủ ký tự mới
     → **font giữ nguyên 100%**, gồm cả font NHÚNG với tiếng Việt.
  2. *Thiếu glyph thật sự* → thay bằng font hệ thống **CÙNG HỌ, đúng đậm/nghiêng**
     (`fontmatch::find_family_font_bytes`: bảng family Windows/macOS, Liberation
     metric-compatible + `fc-match` trên Linux; alias Helvetica→Arial,
     Times→Times New Roman…), có kiểm coverage trước khi nhận.
  3. Bất đắc dĩ mới rơi về font mặc định (`find_font_bytes` — nay có đủ biến thể
     đậm/nghiêng trên cả Linux).
- **Đổi cỡ chữ tại chỗ không đụng font**: nhân matrix `[k,0,0,k, e(1−k), f(1−k)]`
  (neo baseline như Foxit). **Fix bug phóng đại kép**: cỡ chữ nghĩa "hiển thị"
  (đã nhân matrix scale) — trước đây đặt cỡ 20 lên text có matrix scale ×2 ra 40pt.
- `ObjectInfo` thêm `font_family` (đã làm sạch tên PostScript/subset),
  `font_bold`, `font_italic`, `font_embedded` — nguồn từ `name()` (BaseFont) vì
  `family()` của font không nhúng trả tên stub nội bộ PDFium ("Chrom Sans OTF").
- `SetText`/`AddText` nhận `font_family`/`bold`/`italic` dạng Option — **None =
  giữ nguyên**; Some = chủ động đổi (đổi font qua tầng 2).
- Bẫy mới ghi nhận: `set_matrix` của pdfium-render 0.8.37 là alias deprecated
  của `apply_matrix` (NHÂN DỒN, không thay thế) — phải tự dựng matrix delta.

### UI (`main.js` + toolbar)
- **Sửa CẢ DÒNG như Foxit**: double-click gom các run cùng baseline liền kề
  (PDF hay cắt 1 dòng thành nhiều run) → 1 ô sửa cho cả dòng; commit = SetText
  run đầu + Delete các run còn lại (1 batch, 1 nấc undo).
- **WYSIWYG khi gõ**: ô sửa dùng đúng family (CSS xấp xỉ theo `font_family`),
  cỡ, màu, đậm/nghiêng của run gốc; nền trắng che text cũ.
- **Giữ nguyên mặc định**: mọi commit sửa text gửi `null` cho font/cỡ/màu/kiểu
  — chỉ field người dùng chủ động đổi mới được gửi.
- **Toolbar Format kiểu Foxit**: dropdown Font (mặc định "(giữ nguyên: X)"),
  nút **B**/**I** toggle theo kiểu thật của run (đổi biến thể cùng họ), Cỡ chữ,
  Màu — đổi thuộc tính KHÔNG đổi font.
- **Kéo-thả live**: khung đi theo con trỏ ngay khi kéo (move + resize), thả
  chuột mới commit; resize neo góc trên-trái đúng như preview.
- **Dọn file tạm**: mọi file `ff_edit_*.pdf` của phiên sửa được xoá khi thoát
  chế độ/lưu (command `edit_cleanup` — chỉ xoá đúng pattern trong %TEMP%).
- Hint hiển thị font đang chọn: `Tên font · cỡ pt · font nhúng/hệ thống`.

### Test mới (7): `edit_roundtrip.rs` 14 test + `fontmatch` 3 unit
- `set_text_keeps_original_font` — sửa ASCII trên Helvetica base-14 GIỮ NGUYÊN
  "Helvetica" (ép ký tự ngoài charset cũ để không ăn may).
- `set_text_preserves_embedded_font_vietnamese` — **font NHÚNG + tiếng Việt
  giữ nguyên font** (case quan trọng nhất với tài liệu Việt).
- `vietnamese_on_base14_uses_matched_family` — tiếng Việt trên base-14 (không
  có glyph Việt để giữ) match đúng họ metric-compatible (Liberation/Arial),
  không rơi bừa về generic.
- `font_size_change_keeps_font_and_anchors`; `font_size_change_respects_matrix_scale`
  (hồi quy bug phóng đại kép); `bold_override_substitutes_font_and_keeps_text`;
  `line_merge_batch_set_text_plus_delete` (luồng UI gộp dòng).

### Đối chiếu FINAL TARGET & RULE
- Vướng thư viện (PDFium `set_text` re-encode theo subset) → **không cắt giảm**:
  kiểm tra coverage bằng chính font bytes + luật charset/ASCII để tận dụng
  `set_text` an toàn, chỉ thay font khi về mặt vật lý không còn glyph để giữ —
  và khi thay thì match cùng họ như Foxit. Đúng thứ tự a→c của luật.

## Iteration 1 (giữ nguyên bên dưới để tham chiếu)

Phase 4 là tính năng lõi/moat theo `docs/03-roadmap.md`: sửa text & object trực tiếp trên trang — lý do quan trọng nhất để bỏ Foxit.

## 1. Khảo sát trước khi code
- **Foxit UX**: *Edit Text* (bấm đoạn → sửa như Word, reflow, đổi font/cỡ/màu); *Edit Object* (chọn text/ảnh/path → di chuyển/resize/xoay/xoá/thay ảnh, tab Format). Nguồn ở `docs/13-phase4-user-tests.md`.
- **pdfium-render 0.8.37** (đọc source vendored): PDFium đã expose page object cấp cao — KHÔNG cần tự parse content stream. Mỗi `PdfPageTextObject` là 1 **text run** sẵn để sửa (`text()`/`set_text()`/`font()`/`unscaled_font_size()`/fill color/transform). Có `create_text_object`/`create_image_object`/`remove_object_at_index`, `PdfFont::name/family/weight`, `regenerate_content()`. → khả thi cao, vượt kỳ vọng worst-case của roadmap.

## 2. Đã làm

### Engine (`crates/ff-engine/src/edit.rs`)
- `list_objects(input, page, password) -> Vec<ObjectInfo>`: liệt kê object (kind, AABB từ `bounds()`, và với text: nội dung/font/cỡ/màu) — cấp dữ liệu cho overlay UI.
- `apply_edits(input, page, ops, output, password)` với `EditOp`: **Transform** (di chuyển + scale quanh góc dưới-trái), **SetText** (sửa text run — tạo lại bằng FULL font nhúng để tiếng Việt đúng dấu, giữ matrix/cỡ/màu gốc qua `apply_matrix`), **Delete**, **ReplaceImage**, **AddText**, **AddImage**. Thứ tự xử lý giữ index gốc hợp lệ: Transform in-place → chụp dữ liệu object sắp thay → xoá theo index GIẢM DẦN → thêm bản thay thế → thêm object mới → `regenerate_content()` → lưu.
- Tái dùng `annot.rs::find_font_bytes` + `fonts_mut().load_true_type_from_bytes(bytes, true)` (như watermark.rs) cho sửa/thêm text Unicode.
- **Bài học quan trọng (đã ghi memory):** object trả về từ `remove_object_at_index` bị đánh dấu *unowned* → `Drop` gọi `FPDFPageObj_Destroy` gây **SEGFAULT** với PDFium build hiện tại. Phải `std::mem::forget` object đã xoá (đã tách khỏi trang, không cần destroy; rò rỉ nhỏ giải phóng khi đóng document).

### Tauri commands (`app/src-tauri/src/main.rs`)
`edit_list_objects`, `edit_apply` (ghi ra output), `edit_apply_to_temp` (áp ops → file tạm mới, trả path — cho mô hình materialize tức thì ở UI), `edit_preview` (render WYSIWYG), `pick_image`. DTO `ObjectInfoDto` (reuse `RectDto`), `EditOpDto` (tagged theo field `op`).

### UI (`app/src`) — chế độ "✏️ Sửa nội dung"
- Nút toolbar bật chế độ riêng: thay viewport bằng `#editStage` (ảnh trang lớn) + `#editOverlay` (1 box/đối tượng, map pdf→css theo `scale = STAGE_W/pageWidthPt`).
- **Mô hình "materialize tức thì"**: mỗi thao tác áp NGAY vào 1 file tạm mới (`edit_apply_to_temp`) rồi đọc lại object + render ảnh từ đó → index luôn khớp ảnh đang hiện (WYSIWYG thật), không cần tự suy đoán vị trí sau biến đổi.
- Thao tác: click chọn (viền xanh + handle); **double-click text → sửa tại chỗ** (ô input, commit Enter → SetText); đổi **cỡ chữ/màu** ở thanh công cụ cho text đang chọn; **Thêm chữ** (bấm nút → click lên trang → gõ); **Thêm ảnh**/**Thay ảnh** (chọn file → đặt); **Xoá** (nút/phím Delete); **kéo di chuyển / kéo handle resize** (chuột thật).
- **Undo/Redo riêng cho edit** (stack file tạm trước mỗi op), dùng chung 2 nút Hoàn tác/Làm lại + Ctrl+Z/Ctrl+Y khi đang ở chế độ sửa.
- **Lưu**: `edit_apply` ghi file mới rồi `loadDocument`.

## 3. Test tự động (7 mới, `tests/edit_roundtrip.rs`)
list_objects thấy text trang 1; SetText đổi nội dung (text cũ biến mất); SetText **tiếng Việt** round-trip đúng dấu; Delete giảm đúng 1 object; Transform translate dịch bounds ~+50; AddText xuất hiện trong extract_text; AddImage tăng 1 object kind Image. Tổng `ff-engine`: **43/43 xanh**.

## 4. Đã kiểm bằng ảnh (app build release thật)
Bật chế độ Sửa nội dung (overlay 2 đối tượng); double-click dòng 2 → sửa thành "Sửa: nội dung Tiếng Việt" → commit → trang render lại ĐÚNG (dấu chuẩn); Thêm chữ "Dòng chữ MỚI thêm" tại vị trí click; Hoàn tác bỏ chữ vừa thêm (giữ phần sửa); Xoá tiêu đề → còn 1 đối tượng. Ảnh: `tmp-out/phase4-*.png`.

## 5. Còn lại / follow-up (ghi nhận, không phải thiếu sót — Iteration sau)
- **Reflow đoạn nhiều dòng "như Word"** — khoảng cách chính còn lại so với Foxit
  (Iteration 2 đã sửa được CẢ DÒNG; bước tiếp: gom block nhiều dòng theo khoảng
  cách baseline + đo width bằng hmtx của font (ttf-parser đã có) + tự bẻ dòng).
  Xem kế hoạch ở `docs/14-foxit-gap-analysis.md`.
- Xoay/lật/shear/clip object; tab Format nâng cao (viền, opacity, căn lề); z-order arrange; convert text→path. (Đổi font-family/B/I cho object cũ: ĐÃ XONG ở Iteration 2.)
- Đặt ảnh theo đúng tỉ lệ gốc (hiện mặc định 150×112pt, resize bằng kéo handle); preview ảnh trước khi đặt.
- Sửa nhiều trang trong 1 phiên lưu (hiện lưu theo trang đang sửa); spell-check; sửa bảng; link text blocks.
- Mã hoá UI (nợ từ Phase 3).
