FoFreeXit - ban Windows portable (khong can cai dat)
======================================================

Cach chay:
1. Giai nen toan bo file zip nay ra 1 thu muc (giu nguyen ca thu muc, DUNG
   chi lay rieng file .exe ra ngoai - no can pdfium.dll va qpdf.exe canh no).
2. Bam dup vao FoFreeXit.exe.
3. Windows co the hien canh bao "Windows protected your PC" (SmartScreen) vi
   file chua duoc ky so thuong mai - bam "More info" -> "Run anyway".

Da co san (khong can cai):
- pdfium.dll  (render/doc PDF - PDFium, cung engine loi cua Chrome)
- qpdf.exe    (sua file hong, ma hoa/giai ma AES-256)

Can Windows 10/11 co san WebView2 Runtime (da co san tren hau het may Windows
10/11 cap nhat; neu chua co, Windows se tu goi y cai tu Microsoft).

Tinh nang can cai them RIENG (khong bat buoc de dung app):
- OCR (nhan dang chu trong file scan): can cai Tesseract OCR (ban UB
  Mannheim, tick them goi ngon ngu "Vietnamese") - xem docs/20 trong repo.
- Xuat/nhap Word chat luong cao & Office->PDF: can cai LibreOffice - xem
  docs/19 trong repo. Khong co LibreOffice thi Xuat Word van chay duoc bang
  bo chuyen co ban (van du dung).

Day la ban build tu dong tu GitHub Actions, phuc vu MUC DICH DUNG THU/KIEM
TRA. Xem tien do du an tai README.md va thu muc docs/ trong repo.
