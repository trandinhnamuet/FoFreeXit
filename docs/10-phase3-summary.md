# 10 — Tổng kết Phase 3 (Tổ chức trang + Lưu file vững)

> Trạng thái: **lõi HOÀN TẤT**. 36/36 test tự động xanh (`ff-engine`/`ff-cos`/`ff-pdmodel`). UI đã chạy thật và chụp ảnh xác nhận từng tính năng.

## 1. Khảo sát trước khi code

Đọc source `pdfium-render 0.8.37` xác nhận PDFium đã có API cấp cao cho gần hết Phase 3 (copy/insert/delete trang, `set_rotation`, `set_crop`, `watermark(closure)`, nạp font TrueType làm CID font Unicode ngay trong PDFium). Vì vậy **QPDF không còn là động cơ thao tác trang chính** như dự kiến ban đầu — vai trò của nó thu hẹp lại đúng như đã chốt ở `docs/02-tech-stack.md`: lớp an toàn sửa file hỏng + mã hoá lại, PDFium làm hết phần chỉnh trang.

## 2. Đã làm

### Engine (`crates/ff-engine/src`)
- **`organize.rs`**: `PagePlanEntry`/`PageSource` + `build_document(plan)` — 1 hàm lõi dựng document đích từ danh sách "lấy trang nào, từ đâu, xoay/crop ra sao". Insert/Delete/Reorder/Replace/Extract/Merge/Split chỉ là cách dựng `plan` khác nhau, không nhân bản logic. Có `identity_plan`, `delete_pages`, `rotate_pages`, `extract_pages`, `merge_files`, `split_by_page_count`.
- **`watermark.rs`**: `add_watermark`/`add_header_footer` dùng `PdfPages::watermark(closure)` + font TrueType nạp làm CID Unicode (tái dùng `find_font_bytes` từ `annot.rs`) — không cần tự build Type0/CIDFontType2 dict tay như FreeText Phase 2. Token `{page}`/`{total}`/`{date}` cho header/footer; vị trí theo lưới 9 điểm (`Anchor`).
- **`qpdf.rs`**: gọi `qpdf.exe` (binary prebuilt, `scripts/fetch-qpdf.ps1`) qua `std::process::Command`. `repair()` sửa file hỏng trước khi PDFium mở; `encrypt_with_password()` áp lại mã hoá sau khi PDFium/lopdf chỉnh sửa ra bản tạm không mã hoá; `ensure_openable()` thử mở thẳng, lỗi thì tự repair rồi mở lại — dùng ở cả engine và `loadDocument` phía UI.
- Bài học kỹ thuật quan trọng (đã ghi vào memory dự án):
  - `qpdf.exe` hiểu nhầm tiền tố verbatim `\\?\...` (từ `Path::canonicalize()` trên Windows) thành UNC path → phải tự strip trước khi đưa vào argv.
  - Panic trong khi 1 `Pdfium`/`PdfDocument` còn sống sẽ làm "kẹt" (poison) global mutex của `ThreadSafePdfiumBindings` cho hết tiến trình test — mọi test PDFium sau đó báo lỗi `thread_safe.rs:76` không liên quan gì tới bug thật; luôn cô lập test đầu tiên bị fail để tìm nguyên nhân gốc.

### Tauri commands (`app/src-tauri/src/main.rs`)
`ensure_openable`, `organize_identity_plan`, `organize_apply`, `organize_extract`, `organize_merge`, `organize_split`, `watermark_add`, `header_footer_add`, `preview_watermark`, `preview_header_footer`, `pick_dir` (chọn thư mục cho Tách file). DTO `PagePlanEntryDto`/`RectDto` dùng chung 2 chiều (gửi lên + trả về) nên đều có `Serialize + Deserialize`.

### UI (`app/src`)
- **Chế độ "Tổ chức trang"**: nút trên toolbar chính thay `#viewport` bằng lưới thumbnail lớn (`#organizeGrid`), thanh công cụ riêng (`#organizeBar`): Chèn/Xoá/Xoay trái/Xoay phải/Trích/Thay/Trộn file/Tách file/Watermark/Header-Footer/Lưu thay đổi.
- **Multi-select** (click/Ctrl-click/Shift-click) + **kéo-thả đổi thứ tự** (HTML5 drag-and-drop) trên lưới, xoay xem ngay bằng CSS transform trước khi lưu thật.
- **Undo/Redo toàn cục**: mở rộng cơ chế snapshot Phase 2 (chỉ có `annotSpecs`) sang chụp cả `{annotSpecs, pagePlan}` — 1 stack Ctrl+Z/Ctrl+Y duy nhất cho cả chú thích và tổ chức trang.
- **Dialog dùng chung** (`#modalOverlay`/`#modalBox`) cho cả 8 thao tác: Chèn (trang trắng theo cỡ giấy, hoặc từ file + page range), Trích (kèm tuỳ chọn xoá sau khi trích), Thay (theo file + range), Trộn file (thêm/sắp thứ tự/xoá nhiều file), Tách file (theo số trang/file), Watermark (text/cỡ/màu/độ mờ/đậm-nghiêng/góc xoay/lưới 9 vị trí/phạm vi trang — có **xem trước render PDFium thật**), Header/Footer (6 ô trái-giữa-phải × trên-dưới, token chèn nhanh, có xem trước thật).
- **Crop**: tái dùng cơ chế kéo-vẽ hình chữ nhật đã có cho tool "Khung" — thêm nhánh `tool === "crop"` mở dialog 4 số margin (trái/phải/trên/dưới) áp cho trang này hoặc tất cả; áp trực tiếp vào `pagePlan` (chỉ ghi ra file thật khi bấm Lưu).
- **Mở file vững hơn**: `loadDocument` thử mở thẳng, lỗi thì gọi `ensure_openable` (tự repair qua QPDF) rồi mở lại — không cần người dùng tự sửa file.
- Sửa lỗi giao diện phát sinh khi thêm nhiều nút: toolbar/annobar/organizebar tràn ngoài cửa sổ ở độ phân giải hẹp → thêm `flex-wrap: wrap`.

## 3. Test tự động (36/36 xanh, crate `ff-engine`)
- `organize_roundtrip` (8): xoá còn đúng nội dung/thứ tự; xoá hết bị chặn; xoay bền sau khi lưu; trích không đụng file gốc; trộn nhiều file đúng thứ tự; tách đúng số trang/file; crop ghi đúng `/CropBox`; chèn trang trắng đúng kích thước không có text thừa.
- `watermark_roundtrip` (5): watermark hiện trên mọi trang/đúng trang được lọc/tiếng Việt round-trip đúng; header/footer chèn đúng số trang + tổng số trang; ô rỗng bị bỏ qua (không vẽ chữ thừa).
- `qpdf_safety` (7): PDFium từ chối thẳng file bị cắt cụt; repair sửa được; `ensure_openable` tự repair / tự đi qua khi file vốn mở được; fixture mã hoá đòi đúng password; mã hoá round-trip giữ nguyên nội dung.
- Cùng các test Phase 1/2 vẫn xanh (`viewer_features` 6, `big_doc` 3, `render_smoke` 1, `annot_roundtrip` 6).
- `ff-cli` không tự chạy test được trong môi trường này do Windows Smart App Control chặn thực thi binary test mới build (lỗi hệ điều hành, không phải lỗi code) — không ảnh hưởng coverage vì crate này không có test riêng.

## 4. Đã kiểm bằng tay/ảnh (build release, chạy app thật)
- Vào/ra chế độ Tổ chức trang; lưới hiển thị đúng 3 thumbnail thật.
- Chọn trang (viền xanh) + Xoay phải → thumbnail xoay 90° ngay; Hoàn tác → xoay + chọn về lại đúng trạng thái cũ (xác nhận undo/redo toàn cục hoạt động).
- Dialog Chèn trang: chèn trang trắng cỡ Letter vào cuối tài liệu → lưới hiện thêm trang 4 trắng.
- Dialog Watermark: nhập "CONFIDENTIAL", góc xoay 45°, vị trí giữa → bấm Xem trước → ảnh PDFium thật hiện chữ đỏ xoay đúng góc, đúng vị trí, đè lên nội dung trang.
- Dialog Header/Footer: 6 ô + token chèn nhanh hiển thị đúng; Xem trước trả về ảnh PDFium thật (nội dung số trang đã được test-gate ở engine).

## 5. Kiểm duyệt lại luồng hoạt động (2026-06-18) — đã tối ưu

Sau khi rà lại toàn bộ luồng người dùng dùng các chức năng Phase 3, đã sửa:

- **[Hiệu năng — quan trọng] Lưới Tổ chức trang không còn render lại toàn bộ thumbnail khi click chọn/xoay.** Trước đây `orgSelectCard`/`orgRotateSelected` gọi `buildOrganizeGrid()` → xoá sạch lưới và `render_page` lại cho MỌI card (tài liệu N trang → mỗi click = N lần render PDFium + N base64, kèm nhấp nháy). Nay: chọn chỉ đổi class `.selected` (`refreshOrgSelection`), xoay chỉ đổi CSS `transform` của card liên quan; thêm cache thumbnail `state.orgThumbs` (khoá theo `source#srcIndex`) nên cả khi dựng lại lưới (chèn/xoá/đảo) cũng không render lại trang đã render. Reset cache khi mở file mới.
- **[Chưa hợp lý → ĐÃ SỬA THÀNH ĐÚNG, không chỉ cảnh báo] Watermark/Header-Footer giờ tự động áp lên đúng trạng thái `pagePlan` đang xem, không phải file gốc.** Thêm Tauri command `organize_materialize(mainInput, plan, password)` (tái dùng `build_document` có sẵn — không cần engine mới) dựng tạm 1 file PDF theo đúng plan hiện tại; frontend (`materializeBaseInput()`) tự gọi command này khi `planIsDirty()` và dùng file tạm đó làm `input` cho cả Xem trước và Áp dụng của 2 dialog — thay cho cảnh báo "tự đi lưu trước" trước đây. Dialog hiện ghi chú thông tin (không phải cảnh báo chặn) khi đang dùng bản tạm. Đã verify ảnh: xoay trang 1 rồi mở Watermark → Xem trước hiện đúng nội dung trang đã XOAY 90° kèm watermark đè lên — đúng trạng thái đang thấy trên lưới Tổ chức trang, không phải file gốc chưa xoay.
  - Sửa kèm: phạm vi trang nhập trong 2 dialog (`Trang áp dụng`) giờ tính theo `pagePlan.length` (số trang ĐANG XEM) thay vì `pages.length` (số trang file gốc) — 2 số này khác nhau ngay khi đã chèn/xoá trang.
- **[Hiệu năng] `split_by_page_count` mở file nguồn đúng 1 lần** thay vì gọi `build_document` mỗi phần (mỗi lần mở/parse lại cả file). Tách file nhiều trang nhanh hơn rõ rệt. Test `split_by_page_count_produces_correct_chunks` vẫn xanh.
- **[Bẫy verify đã gặp, ghi vào memory dự án]** App Tauri là workspace TÁCH RIÊNG khỏi `crates/*` — `cargo build --release` chạy từ gốc repo chỉ build `crates/*`, KHÔNG build app (dễ verify nhầm trên exe cũ). Phải dùng `cargo build --release --manifest-path app/src-tauri/Cargo.toml`.

## 6. Còn lại / follow-up (chưa làm)
- **Mã hoá trong UI:** engine có sẵn `encrypt_with_password`/`ensure_openable` nhưng frontend luôn truyền `password: null`, chưa có hộp nhập mật khẩu lúc mở, chưa nối lại mã hoá sau khi lưu. → File có mật khẩu thực tế chưa mở/lưu được qua UI (DoD "ghi không làm hỏng file mã hoá" mới đạt ở tầng engine/test, chưa ở tầng app). Cần: prompt mật khẩu khi `ensure_openable`/mở lỗi do mã hoá; sau khi `organize_apply`, nếu file gốc có mật khẩu thì gọi `encrypt_with_password` áp lại. Cân nhắc quy mô lớn (đụng tới mọi command mở/render/search) — để làm riêng, không gộp vào lần rà soát này.
- Live preview render thật cho Insert/Extract/Replace/Crop (hiện Crop dùng số liệu margin nhập tay; Insert/Extract/Replace không có preview).
- Tách theo outline cấp 1 (hiện chỉ có "mỗi N trang").
- Watermark ảnh (hiện chỉ có watermark text).
- Incremental save thật giữ chữ ký số (để dành Phase 5 theo đúng kế hoạch ban đầu).
