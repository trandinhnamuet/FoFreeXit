# 17 — Tổng kết Phase 6 (Form — AcroForm)

> Trạng thái: **HOÀN TẤT lõi.** Liệt kê / điền / tạo field, flatten,
> export/import FDF + export CSV. 5 test round-trip mới, tổng engine **67/67
> xanh** (ngoài 2 fixture qpdf Linux). Chi tiết & giới hạn ở mục 4–5.

## 1. Khảo sát Foxit (chuẩn nghiệm thu)
- **Fill & Sign / Form**: điền field có sẵn (text/checkbox/radio/combo/list),
  lưu giữ dữ liệu; **Flatten** biến field thành nội dung tĩnh.
- **Prepare Form**: tạo field mới (text/checkbox/radio/dropdown/button).
- **Import/Export Form Data**: FDF/XFDF/CSV để trao đổi dữ liệu.

## 2. Lựa chọn kỹ thuật
- **Đọc/ghi cấu trúc AcroForm bằng `lopdf`** (thao tác tầng PDF object) —
  portable, không cần khởi tạo form-environment của PDFium (vốn phức tạp và
  phụ thuộc build). **Flatten dùng PDFium** (`page.flatten()`).
- Điền field đặt `NeedAppearances=true` để MỌI viewer tự dựng lại appearance —
  cách tương thích rộng nhất, không phải tự vẽ appearance stream cho từng loại.

## 3. Đã làm — Engine (`form.rs`)
- **`list_form_fields`**: duyệt cây `/AcroForm/Fields` (đệ quy /Kids là field
  con), suy tên đầy đủ (fully-qualified), phân loại theo `/FT` + cờ `/Ff`
  (checkbox vs radio vs pushbutton; combo vs list), đọc `/V`, options, on-state
  (tên trạng thái bật khác "Off" trong /AP /N), trang chứa widget + rect.
  Kế thừa `/FT` từ cha cho field lá không tự khai.
- **`fill_form_fields`**: text/combo/list đặt `/V` (UTF-16BE cho tiếng Việt),
  xoá `/AP` để dựng lại; checkbox/radio đặt `/V` + `/AS` widget về on-state;
  bật NeedAppearances.
- **`flatten_form`**: PDFium flatten từng trang → in field vào nội dung, bỏ
  tương tác.
- **`create_form_fields`**: tạo widget + field text/checkbox/combo (viền + nền
  nhạt, /DA font mặc định), gắn vào `/Annots` trang + `/AcroForm/Fields`, tạo
  AcroForm nếu chưa có.
- **FDF & CSV**: `export_fdf` (literal cho ASCII, hex UTF-16BE cho Unicode —
  round-trip tiếng Việt), `parse_fdf`/`import_fdf` (parser đọc cả literal `(..)`
  và hex `<..>`), `export_csv`.

## 4. Tauri + UI (thanh "📝 Form")
- Commands: `form_list`, `form_fill`, `form_flatten`, `form_create`,
  `form_export` (FDF/CSV theo đuôi), `form_import_fdf`; picker
  `pick_save_data`/`pick_fdf`. Mọi command tự chuẩn hoá qua qpdf khi lopdf
  không đọc thẳng trailer phi chuẩn.
- **📋 Điền form**: modal liệt kê mọi field với input đúng loại (text→ô nhập,
  checkbox→tick, combo→select) → Lưu & áp dụng ra file mới.
- **➕ Thêm field**: dialog chọn loại/tên/trang/vị trí/cỡ (+ options cho combo).
- **🧷 Flatten**, **⬆ Xuất FDF / CSV**, **⬇ Nhập FDF**.
- Không dùng prompt/alert; đếm số field hiển thị trên nút.

## 5. Test (5 mới → engine 67/67)
- `create_and_list_fields` — tạo 3 field (text/checkbox/combo) rồi liệt kê
  đúng loại + options + trang + rect.
- `fill_text_checkbox_combo_round_trips` — điền (text tiếng Việt, checkbox bật,
  combo chọn) rồi đọc lại đúng giá trị.
- `fdf_export_import_round_trip` — điền → export FDF → parse thấy giá trị →
  import vào fixture rỗng → giá trị được điền lại (tiếng Việt đúng).
- `export_csv_has_rows` — CSV có header + dòng name,value.
- `flatten_removes_interactive_fields` — sau flatten không còn field tương tác.

## 6. Giới hạn (v-sau, không chặn dùng)
- Tạo field mới: hỗ trợ text/checkbox/combo; radio-group nhiều nút, list-box,
  pushbutton (action) chưa tạo (đọc/điền thì được).
- Chưa vẽ appearance stream riêng → phụ thuộc NeedAppearances (Chrome/Foxit/
  Acrobat đều dựng; vài viewer tối giản có thể không). Flatten khắc phục khi
  cần bản “cứng”.
- Chưa XFDF (XML); mới FDF + CSV. Chưa validate/format (số, ngày) theo JS của
  field. Điền trên UI qua danh sách (chưa overlay click trực tiếp lên trang —
  cân nhắc cho vòng sau như edit-stage).
