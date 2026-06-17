# 09 — Checklist test thủ công Phase 2 (Annotate)

Phần kéo-chuột không tự động test được trong WebView2 (xem ghi chú ở [08-phase2-summary.md](08-phase2-summary.md)), nên cần bạn thử bằng chuột thật.

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| G1 | Chọn **Tô sáng**, kéo qua một dòng chữ | Vệt vàng phủ đúng vùng kéo | |
| G2 | Chọn **Gạch chân**, kéo qua chữ | Đường kẻ dưới chữ | |
| G3 | Chọn **Gạch ngang**, kéo qua chữ | Đường kẻ giữa chữ | |
| G4 | Chọn **Khung**, kéo một vùng | Khung chữ nhật | |
| G5 | Đổi **Màu** rồi vẽ | Chú thích đúng màu mới | |
| G6 | Chọn **Text box**, kéo vùng, gõ chữ | Hộp chữ hiện nội dung | |
| G7 | Chọn **Ghi chú**, bấm lên trang, gõ chữ | Icon ghi chú 📝 (rê chuột thấy nội dung) | |
| G8 | Tab **Chú thích** (sidebar) | Liệt kê các chú thích vừa vẽ; bấm để nhảy trang; ✕ để xoá | |
| G9 | Bấm **Lưu chú thích**, chọn nơi lưu | Tạo file PDF mới; app mở lại file đó | |
| G10 | Mở file vừa lưu bằng **Foxit/Adobe/Chrome** | Annotation hiển thị đúng ở phần mềm khác | |
| G11 | Vẽ trên file **nhiều trang**, cuộn qua lại | Chú thích đúng trang, đúng vị trí khi cuộn/zoom | |
| G12 | Vẽ rồi **zoom** in/out | Chú thích co giãn theo, vẫn đúng chỗ | |

## Mẫu phản hồi
```
[Mã] (vd G10) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Đặc biệt cần biết **G10** (file lưu ra mở ở phần mềm khác có đúng không) và **G9** (lưu có thành công không), vì đó là đường GHI file. Engine đã test round-trip nội bộ, nhưng xác nhận cross-viewer của bạn rất giá trị.
