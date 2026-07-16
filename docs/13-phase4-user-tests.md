# 13 — Checklist test thủ công Phase 4 (Sửa nội dung)

Kéo-chuột (di chuyển/resize) không tự động test đầy đủ trong WebView2 (xem `docs/08`), nên cần bạn thử bằng chuột thật. Đặc biệt cần xác nhận **lưu ra file mở lại đúng** và **tiếng Việt đúng dấu** ở phần mềm khác.

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E1 | Bấm **✏️ Sửa nội dung** | Viewport đổi thành trang lớn + khung mờ quanh từng đối tượng (text/ảnh) | |
| E2 | Bấm 1 lần vào 1 khung text | Khung viền xanh (đã chọn); ô Cỡ chữ + Màu bật lên | |
| E3 | Bấm đúp (double-click) 1 khung text | Hiện ô sửa tại chỗ với nội dung cũ | |
| E4 | Sửa thành chữ có dấu tiếng Việt, Enter | Trang render lại đúng nội dung mới, **đúng dấu** | |
| E5 | Chọn text rồi đổi **Cỡ chữ** | Chữ đổi kích thước | |
| E6 | Chọn text rồi đổi **Màu** | Chữ đổi màu | |
| E7 | Bấm **Thêm chữ** → click lên trang → gõ → Enter | Chữ mới xuất hiện đúng chỗ | |
| E8 | Bấm **Thêm ảnh** → chọn ảnh → click lên trang | Ảnh xuất hiện tại chỗ click | |
| E9 | Chọn 1 ảnh → **Thay ảnh** → chọn ảnh khác | Ảnh được thay, giữ khung cũ | |
| E10 | Chọn 1 đối tượng → **Kéo** để di chuyển | Đối tượng dời theo chuột | |
| E11 | Chọn 1 đối tượng → kéo **ô vuông góc** để resize | Đối tượng to/nhỏ lại | |
| E12 | Chọn 1 đối tượng → phím **Delete** (hoặc nút Xoá) | Đối tượng biến mất | |
| E13 | **Hoàn tác (Ctrl+Z)** sau vài thao tác | Lùi lại từng bước đúng | |
| E14 | **Làm lại (Ctrl+Y)** | Tiến lại bước vừa hoàn tác | |
| E15 | **Lưu thay đổi nội dung** → chọn nơi lưu | Tạo file PDF mới; app mở lại file đó | |
| E16 | Mở file đã lưu ở **Foxit/Adobe/Chrome** | Nội dung đã sửa hiển thị đúng (text, ảnh, **tiếng Việt đúng dấu**), file không hỏng | |

## Bổ sung Iteration 2 — GIỮ FONT + sửa cả dòng (chuẩn Foxit)
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E17 | Bấm đúp 1 dòng text bị cắt thành nhiều mảnh (PDF xuất từ Word thường vậy) | Ô sửa phủ **CẢ DÒNG** (không chỉ 1 mảnh), nội dung ghép đủ | |
| E18 | Sửa 1 từ trong dòng, Enter | **Font giữ nguyên y hệt** phần còn lại của trang (so bằng mắt + panel chọn hiện tên font cũ) | |
| E19 | Trong ô sửa đang gõ | Chữ trong ô hiển thị **đúng font/cỡ/màu/đậm-nghiêng** của dòng (WYSIWYG khi gõ, không phải font hệ thống chung chung) | |
| E20 | Chọn text → chỉ đổi **Cỡ chữ** hoặc **Màu** | Font KHÔNG đổi, nội dung không đổi, vị trí neo không trôi | |
| E21 | Chọn text → bấm **B** (Đậm) | Chữ thành đậm **cùng họ font** (Times → Times Bold, không nhảy sang Arial) | |
| E22 | Chọn text → đổi **Font** trong dropdown | Chữ đổi đúng family chọn; dropdown mặc định hiển thị "(giữ nguyên: <font gốc>)" | |
| E23 | Kéo di chuyển / kéo handle resize | Khung **đi theo con trỏ NGAY khi kéo** (live), thả chuột mới ghi nhận | |
| E24 | Sửa text tiếng Việt trên file có font nhúng (file Word xuất PDF) | Đúng dấu **và giữ nguyên font nhúng** (mở lại ở Foxit xem tên font trong Edit) | |
| E25 | Sửa nhiều lần rồi thoát chế độ sửa (không lưu) | Không còn file `ff_edit_*.pdf` rác trong thư mục %TEMP% | |

## Bổ sung — Sửa đoạn hoàn chỉnh với PDF Word-export (khắc phục theo phản hồi thật)
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E34 | Mở PDF xuất từ Word (tiêu đề bị cắt thành từng từ/ký tự), bật chế độ Sửa | Overlay hiện **1 khung mỗi DÒNG** (không phải hàng chục ô nhỏ từng từ) | |
| E35 | Đúp vào tiêu đề 2 dòng (kể cả đúp vào KHE giữa 2 từ) | Ô sửa mở cho **CẢ ĐOẠN 2 DÒNG, giữ nguyên xuống dòng** | |
| E36 | Trong ô sửa: click chuột chỗ khác của đoạn để đổi vị trí con trỏ | Con trỏ di chuyển bình thường, **KHÔNG thoát edit** | |
| E37 | Sửa vài chữ rồi Ctrl+Enter | Chữ mới thay SẠCH chữ cũ (không chồng chữ), tiêu đề căn giữa **vẫn căn giữa** | |
| E38 | Khi ô sửa đang mở | Ô che kín chữ cũ bên dưới (không thấy 2 lớp chữ) | |

## Bổ sung — Đúp chuột vào chữ từ viewer thường (chuẩn Foxit Editor)
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E31 | Đang XEM bình thường (không bật chế độ sửa), **đúp chuột vào 1 dòng chữ** | Tự vào chế độ Sửa nội dung đúng trang đó và Ô SỬA mở sẵn ngay tại dòng/đoạn vừa đúp | |
| E32 | Đúp chuột vào VÙNG TRỐNG của trang | Không có gì xảy ra (như Foxit) | |
| E33 | Đang cầm công cụ chú thích (highlight/note...) rồi đúp vào chữ | KHÔNG nhảy vào chế độ sửa (công cụ chú thích được ưu tiên) | |

## Bổ sung Iteration 3 — Sửa cả đoạn với reflow "như Word"
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E26 | Bấm đúp vào 1 đoạn văn NHIỀU dòng | Textarea phủ CẢ ĐOẠN, nội dung ghép đủ các dòng, đúng font/cỡ/màu | |
| E27 | Thêm 1 câu dài vào giữa đoạn → Ctrl+Enter (hoặc bấm ra ngoài) | Chữ tự bẻ dòng lại trong đúng bề rộng khối, không tràn phải, khoảng cách dòng giữ nguyên, **font giữ nguyên** | |
| E28 | Trong textarea bấm Enter giữa chừng rồi áp dụng | Vị trí Enter thành xuống dòng cứng (đoạn tách dòng tại đó) | |
| E29 | Xoá bớt chữ cho đoạn ngắn lại → áp dụng | Đoạn co lại còn ít dòng hơn, không sót dòng cũ | |
| E30 | Hoàn tác (Ctrl+Z) sau 1 lần reflow | Cả đoạn quay về nguyên trạng trong 1 bước | |

## Mẫu phản hồi
```
[Mã] (vd E16) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Quan trọng nhất: **E4** (sửa tiếng Việt), **E15/E16** (lưu + mở lại cross-viewer) — đường GHI nội dung là rủi ro nhất. Engine đã test round-trip nội bộ (gồm tiếng Việt), nhưng xác nhận cross-viewer của bạn rất giá trị.

> Giới hạn đã biết (Iteration 1): sửa text ở mức **dòng/run** (chưa reflow cả đoạn như Word); thêm ảnh mặc định khung 150×112pt (kéo handle để chỉnh); lưu áp cho trang đang sửa. Xem `docs/12-phase4-summary.md` mục 5.
