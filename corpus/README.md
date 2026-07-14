# Corpus PDF mẫu

Bộ file PDF dùng để test & hồi quy. Sẽ mở rộng dần theo roadmap.

| File | Mô tả | Dùng cho |
|------|-------|----------|
| `hello.pdf` | PDF tối giản 1 trang, text Helvetica, xref hợp lệ | Smoke test render (Phase 1) |
| `sample-multipage.pdf` | 3 trang Letter, nội dung biết trước + outline (Chapter 1/2/3 → trang 0/1/2) | Test page_dims, extract_text, search, outline; demo viewer |
| `big-1000.pdf` | 1000 trang ("Trang N"); trang index 500 có "ZZMARKER" | Stress-test hiệu năng/lazy-load (mở 55ms, render 110ms, search 79ms) |
| `encrypted.pdf` | `sample-multipage.pdf` mã hoá AES-256 qua qpdf, user password `fofreexit`, owner password `owner-fofreexit` | Test QPDF encrypted-safe-save (Phase 3), bảo mật (Phase 5) |
| `corrupt-truncated.pdf` | `sample-multipage.pdf` bị cắt cụt 200 byte cuối (mất trailer/startxref) — PDFium đọc thẳng bị `FormatError`, qpdf tự dựng lại xref và đọc lại được đủ 3 trang | Test QPDF repair-trước-khi-mở (Phase 3) |

## Cần bổ sung (các phase sau)
- File nhiều trang (100+, 1000+) — hiệu năng & lazy load.
- File scan (ảnh) — OCR (Phase 7).
- File tiếng Việt (font nhúng, tổ hợp dấu), CJK, RTL — edit text (Phase 4).
- File có AcroForm — form (Phase 6).

> Lưu ý bản quyền: chỉ thêm file tự tạo hoặc có giấy phép cho phép phân phối.
