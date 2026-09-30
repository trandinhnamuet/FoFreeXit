// Bộ icon SVG line 24px (stroke currentColor, 1.6px). <i data-icon="name"></i> → SVG.
// Vẽ tay trên lưới 24×24, đệm 2px, bo góc thống nhất (r=2 cho khung, r≈1.2 cho chi tiết nhỏ).
const ICONS = (() => {
  // Trang tài liệu (góc gấp) — dùng chung cho file-* / page-*.
  const PAGE = '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>';
  // Trang "khuyết" góc dưới-phải để chừa chỗ cho badge (không đè nét).
  const PAGE_B = '<path d="M11 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v3"/><path d="M14 3v5h5"/>';
  // Khung ảnh khuyết góc dưới-phải (cho image-add / image-replace).
  const FRAME_B = '<path d="M21 12V6a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h7"/><circle cx="8.5" cy="9" r="1.5"/><path d="M3 16.5l4.5-4.5 4 4 2-2"/>';
  const dot = (x, y, r = 0.9) => `<circle cx="${x}" cy="${y}" r="${r}" fill="currentColor" stroke="none"/>`;
  const bar = (x, y, w, h, r = 1) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${r}" fill="currentColor" stroke="none"/>`;

  return {
    // ---- Caption Windows 11 (hiển thị 16px) ----
    "win-min": '<path d="M5 12h14"/>',
    "win-max": '<rect x="5.5" y="5.5" width="13" height="13" rx="1.5"/>',
    "win-restore": '<rect x="5" y="8" width="11" height="11" rx="1.5"/><path d="M8 8V6.5A1.5 1.5 0 0 1 9.5 5h8A1.5 1.5 0 0 1 19 6.5v8a1.5 1.5 0 0 1-1.5 1.5H16"/>',
    "win-close": '<path d="M6 6l12 12M18 6L6 18"/>',

    // ---- Điều hướng ----
    "menu": '<path d="M4 7h16M4 12h16M4 17h16"/>',
    "chevron-up": '<path d="M6 15l6-6 6 6"/>',
    "chevron-down": '<path d="M6 9l6 6 6-6"/>',
    "chevron-left": '<path d="M15 6l-6 6 6 6"/>',
    "chevron-right": '<path d="M9 6l6 6-6 6"/>',
    "page-first": '<path d="M17 6l-6 6 6 6"/><path d="M7 6v12"/>',
    "page-last": '<path d="M7 6l6 6-6 6"/><path d="M17 6v12"/>',

    // ---- Tệp ----
    "save": '<path d="M5 3h11l5 5v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M7.5 3v4.5h7V3"/><path d="M7 21v-6.5a1 1 0 0 1 1-1h8a1 1 0 0 1 1 1V21"/>',
    "folder-open": '<path d="M3.5 18.5V6a2 2 0 0 1 2-2h3.6l2 2H17a2 2 0 0 1 2 2v2"/><path d="M3.5 18.5L6.3 11.7a1.2 1.2 0 0 1 1.1-.7H20.5a1 1 0 0 1 .93 1.37l-2.6 6.4a1.2 1.2 0 0 1-1.1.73H4.5a1 1 0 0 1-1-1z"/>',
    "quit": '<path d="M10 3.5H6a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h4"/><path d="M10 12h10.5"/><path d="M16.5 8l4 4-4 4"/>',
    "export": '<path d="M4 14.5V18a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3.5"/><path d="M12 15V4"/><path d="M7.5 8.5L12 4l4.5 4.5"/>',
    "import": '<path d="M4 14.5V18a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3.5"/><path d="M12 4v11"/><path d="M7.5 10.5L12 15l4.5-4.5"/>',
    "file-text": PAGE + '<path d="M8.5 9h3M8.5 12.5h7M8.5 16h7"/>',
    "file-word": PAGE + '<path d="M8 11.5l1.5 6 2.5-4.5 2.5 4.5 1.5-6"/>',
    "file-pdf": PAGE + '<path d="M8.5 9h3"/>' + bar(7.5, 12, 9, 5.5, 1.2),
    "file-image": PAGE + '<circle cx="9.5" cy="11" r="1.4"/><path d="M7.5 18l3.2-3.4 2.3 2.3 1.5-1.5 2 2.1"/>',
    "file-info": PAGE_B + '<circle cx="17" cy="17" r="4.5"/><path d="M17 16.3v2.9"/>' + dot(17, 14.6, 0.75),
    "file-export": '<path d="M16 9V7.5L11.5 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2v-4"/><path d="M11.5 3v4.5H16"/><path d="M9.5 12H21"/><path d="M18 9l3 3-3 3"/>',
    "file-import": '<path d="M8 9V5a2 2 0 0 1 2-2h5.5L20 7.5V19a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-4"/><path d="M15.5 3v4.5H20"/><path d="M3 12h10.5"/><path d="M10.5 9l3 3-3 3"/>',

    // ---- Trang ----
    "page-insert": PAGE_B + '<path d="M17.5 14v7M14 17.5h7"/>',
    "page-delete": PAGE_B + '<path d="M14.5 14.5l5 5M19.5 14.5l-5 5"/>',
    "page-extract": PAGE_B + '<path d="M13.5 20.5l7-7"/><path d="M15.5 13.5h5v5"/>',
    "page-replace": PAGE_B + '<path d="M14 15h7l-2.2-2.2"/><path d="M21 19h-7l2.2 2.2"/>',
    "rotate-left": '<rect x="10.5" y="9" width="9.5" height="12" rx="1.6"/><path d="M15 4.5H10A5 5 0 0 0 5 9.5V13"/><path d="M3 11l2 2 2-2"/>',
    "rotate-right": '<rect x="4" y="9" width="9.5" height="12" rx="1.6"/><path d="M9 4.5h5a5 5 0 0 1 5 5V13"/><path d="M17 11l2 2 2-2"/>',
    "merge": '<rect x="3" y="3" width="7" height="7.5" rx="1.4"/><rect x="3" y="13.5" width="7" height="7.5" rx="1.4"/><path d="M10 6.75h1a3 3 0 0 1 3 3V12M10 17.25h1a3 3 0 0 0 3-3V12"/><path d="M14 12h7"/><path d="M18.5 9.5L21 12l-2.5 2.5"/>',
    "split": '<path d="M5 9.5V5a2 2 0 0 1 2-2h7l5 5v1.5"/><path d="M14 3v5h5"/><path d="M5 14.5V19a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4.5"/><path d="M2.5 12h2.5M8 12h3M13.5 12h3M19 12h2.5"/>',
    "thumbs": '<rect x="4" y="3" width="6.5" height="8" rx="1.3"/><rect x="13.5" y="3" width="6.5" height="8" rx="1.3"/><rect x="4" y="13" width="6.5" height="8" rx="1.3"/><rect x="13.5" y="13" width="6.5" height="8" rx="1.3"/>',
    "watermark": PAGE + '<path d="M12 10.3s-3.2 3.4-3.2 5.6a3.2 3.2 0 0 0 6.4 0c0-2.2-3.2-5.6-3.2-5.6z"/>',
    "header-footer": '<rect x="4.5" y="3" width="15" height="18" rx="2"/>' + bar(7.5, 5.8, 9, 2.2, 1.1) + '<path d="M8 11.2h8M8 13.8h5.5"/>' + bar(7.5, 16.3, 9, 2.2, 1.1),
    // Page Marks / tổ chức trang mở rộng (pagex).
    "page-background": PAGE + '<path d="M7.5 21L19 9.5M5 17.5l9-9M5 12.5L10.5 7"/>',
    "bates": PAGE_B + '<path d="M15.5 13.5l-1 7M19.5 13.5l-1 7M13.5 15.8h7M13 18.3h7"/>',
    "page-duplicate": '<rect x="8.5" y="7" width="11" height="14" rx="2"/><path d="M15.5 7V5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h2"/><path d="M14 11.5v5M11.5 14h5"/>',
    "page-reverse": '<rect x="3.5" y="4" width="7" height="9" rx="1.4"/><rect x="13.5" y="11" width="7" height="9" rx="1.4"/><path d="M14 4.5h3a3 3 0 0 1 3 3V8"/><path d="M18.3 6.3L20 8l1.7-1.7"/><path d="M10 19.5H7a3 3 0 0 1-3-3V16"/><path d="M5.7 17.7L4 16l-1.7 1.7"/>',
    "chevron-down-sm": '<path d="M8 10l4 4 4-4"/>',
    "plus": '<path d="M12 5v14M5 12h14"/>',
    "refresh": '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4v4h-4"/>',
    "copy": '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M15 9V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h4"/>',
    "crop": '<path d="M6 2.5V16a2 2 0 0 0 2 2h13.5"/><path d="M2.5 6H16a2 2 0 0 1 2 2v13.5"/>',
    "compress": '<path d="M4 12h16"/><path d="M12 3v5.5M9 5.8l3 2.7 3-2.7"/><path d="M12 21v-5.5M9 18.2l3-2.7 3 2.7"/>',
    "flatten": '<path d="M12 3l8.5 4.5L12 12 3.5 7.5z"/><path d="M3.5 12L12 16.5 20.5 12"/><path d="M3.5 16.5L12 21l8.5-4.5"/>',

    // ---- So sánh tài liệu ----
    "compare": '<rect x="3" y="3.5" width="7.5" height="17" rx="1.6"/><rect x="13.5" y="3.5" width="7.5" height="17" rx="1.6"/><path d="M5.5 8h2.5M5.5 11.5h2.5M16 8h2.5M16 15h2.5"/>' + bar(15.5, 10.6, 3.5, 2, 0.6),
    "swap": '<path d="M4 8.5h15l-3.5-3.5"/><path d="M20 15.5H5l3.5 3.5"/>',

    // ---- Xem ----
    "zoom-in": '<circle cx="10.5" cy="10.5" r="6.5"/><path d="M20 20l-4.6-4.6"/><path d="M8 10.5h5M10.5 8v5"/>',
    "zoom-out": '<circle cx="10.5" cy="10.5" r="6.5"/><path d="M20 20l-4.6-4.6"/><path d="M8 10.5h5"/>',
    "zoom-plus": '<circle cx="12" cy="12" r="8.5"/><path d="M8.5 12h7M12 8.5v7"/>',
    "zoom-minus": '<circle cx="12" cy="12" r="8.5"/><path d="M8.5 12h7"/>',
    "fit-width": '<path d="M3.5 5v14M20.5 5v14"/><path d="M7 12h10"/><path d="M9.5 9.5L7 12l2.5 2.5M14.5 9.5L17 12l-2.5 2.5"/>',
    "fit-page": '<path d="M3 7.5V5a2 2 0 0 1 2-2h2.5M16.5 3H19a2 2 0 0 1 2 2v2.5M21 16.5V19a2 2 0 0 1-2 2h-2.5M7.5 21H5a2 2 0 0 1-2-2v-2.5"/><rect x="8" y="7" width="8" height="10" rx="1.3"/>',
    "fullscreen": '<path d="M3 8.5V5a2 2 0 0 1 2-2h3.5M15.5 3H19a2 2 0 0 1 2 2v3.5M21 15.5V19a2 2 0 0 1-2 2h-3.5M8.5 21H5a2 2 0 0 1-2-2v-3.5"/>',
    "eye": '<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>',
    "sidebar": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/><path d="M5.3 8h1.4M5.3 11h1.4"/>',
    "outline": '<circle cx="5" cy="6" r="1.6"/><path d="M5 7.6V16a2 2 0 0 0 2 2h2M5 10a2 2 0 0 0 2 2h2"/><path d="M9.5 6h10.5M12.5 12h7.5M12.5 18h7.5"/>',
    "search": '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.4-4.4"/>',
    "search-doc": PAGE_B + '<circle cx="16" cy="16" r="3.3"/><path d="M18.5 18.5L21 21"/>',
    "comments": '<path d="M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-7.5L6 20.5V17a2 2 0 0 1-2-2z"/><path d="M8 8.8h8M8 12.3h5"/>',

    // ---- Chọn / sửa ----
    "cursor": '<path d="M5 3.5l5.7 15.8 2.3-6.3 6.3-2.3z"/><path d="M13.2 13.2l5.6 5.6"/>',
    "edit-text": '<path d="M4 5h11M9.5 5v13.5"/><path d="M14.3 20.2l.6-2.9 5.3-5.3a1.4 1.4 0 0 1 2 2l-5.3 5.3z"/>',
    "text-add": '<path d="M4 5h11M9.5 5v13.5"/><path d="M18 13.5v7M14.5 17h7"/>',
    "textbox": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M8 8.5h8M12 8.5v8"/>',
    "image-add": FRAME_B + '<path d="M18 14.5v7M14.5 18h7"/>',
    "image-replace": FRAME_B + '<path d="M14.5 15.5h6.5l-2-2"/><path d="M21 19.5h-6.5l2 2"/>',
    "trash": '<path d="M4 6.5h16"/><path d="M9 6.5V4.8A1.8 1.8 0 0 1 10.8 3h2.4A1.8 1.8 0 0 1 15 4.8v1.7"/><path d="M6 6.5l.9 12.6A2 2 0 0 0 8.9 21h6.2a2 2 0 0 0 2-1.9L18 6.5"/><path d="M10 10.5v6.5M14 10.5v6.5"/>',
    // ---- Edit Object (editx): hình vẽ, lật, trích ảnh, căn lề, định dạng, sắp lớp ----
    "shapes": '<rect x="3" y="11" width="9" height="9" rx="1.6"/><circle cx="16" cy="8" r="5"/>',
    "shape-rect": '<rect x="3.5" y="6" width="17" height="12" rx="1"/>',
    "shape-roundrect": '<rect x="3.5" y="6" width="17" height="12" rx="4"/>',
    "shape-ellipse": '<ellipse cx="12" cy="12" rx="8.5" ry="6"/>',
    "shape-line": '<path d="M5 19L19 5"/>',
    "shape-arrow": '<path d="M5 19L18 6"/><path d="M10.5 5.5H18.5V13.5"/>',
    "flip-h": '<path d="M12 3v18" stroke-dasharray="2 2.2"/><path d="M9 7L3.5 17H9z"/><path d="M15 7l5.5 10H15z"/>',
    "flip-v": '<path d="M3 12h18" stroke-dasharray="2 2.2"/><path d="M7 9L17 3.5V9z"/><path d="M7 15l10 5.5V15z"/>',
    "image-export": FRAME_B + '<path d="M18 21.5v-7M15 17.5l3-3 3 3"/>',
    "align-text-left": '<path d="M4 6h16M4 10h10M4 14h16M4 18h10"/>',
    "align-text-center": '<path d="M4 6h16M7 10h10M4 14h16M7 18h10"/>',
    "align-text-right": '<path d="M4 6h16M10 10h10M4 14h16M10 18h10"/>',
    "stroke-color": '<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="8" y="8" width="8" height="8" rx="1"/>',
    "fill-color": '<path d="M5 12.5L12 5.5l6.5 6.5L11.5 19z"/><path d="M8.5 9L4 4.5"/><path d="M19.5 15.5s1.5 2 1.5 3a1.5 1.5 0 0 1-3 0c0-1 1.5-3 1.5-3z"/>',
    "arrange": '<rect x="8" y="3" width="13" height="10" rx="1.6"/><path d="M5.5 7H4.6A1.6 1.6 0 0 0 3 8.6v10.8A1.6 1.6 0 0 0 4.6 21h10.8a1.6 1.6 0 0 0 1.6-1.6v-3.9"/>',
    "bring-front": '<rect x="7" y="7" width="10" height="10" rx="1.4" fill="currentColor" fill-opacity=".25"/><path d="M4 11V5a1 1 0 0 1 1-1h6M13 20h6a1 1 0 0 0 1-1v-6"/>',
    "bring-forward": '<rect x="9" y="9" width="11" height="11" rx="1.4" fill="currentColor" fill-opacity=".25"/><path d="M15 5V4a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h1"/>',
    "send-backward": '<rect x="9" y="9" width="11" height="11" rx="1.4"/><path d="M15 9V4a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h5" fill="currentColor" fill-opacity=".25"/>',
    "send-back": '<rect x="7" y="7" width="10" height="10" rx="1.4"/><path d="M4 11V5a1 1 0 0 1 1-1h6M13 20h6a1 1 0 0 0 1-1v-6"/><path d="M9.5 12h5" />',
    "align-objects": '<path d="M4 3v18"/><rect x="7" y="5.5" width="12" height="5" rx="1.2"/><rect x="7" y="13.5" width="7" height="5" rx="1.2"/>',
    "align-left": '<path d="M4 3v18"/><rect x="7" y="5.5" width="12" height="5" rx="1.2"/><rect x="7" y="13.5" width="7" height="5" rx="1.2"/>',
    "align-center": '<path d="M12 3v18"/><rect x="5" y="5.5" width="14" height="5" rx="1.2"/><rect x="8" y="13.5" width="8" height="5" rx="1.2"/>',
    "align-right": '<path d="M20 3v18"/><rect x="5" y="5.5" width="12" height="5" rx="1.2"/><rect x="10" y="13.5" width="7" height="5" rx="1.2"/>',
    "align-top": '<path d="M3 4h18"/><rect x="5.5" y="7" width="5" height="12" rx="1.2"/><rect x="13.5" y="7" width="5" height="7" rx="1.2"/>',
    "align-middle": '<path d="M3 12h18"/><rect x="5.5" y="5" width="5" height="14" rx="1.2"/><rect x="13.5" y="8" width="5" height="8" rx="1.2"/>',
    "align-bottom": '<path d="M3 20h18"/><rect x="5.5" y="5" width="5" height="12" rx="1.2"/><rect x="13.5" y="10" width="5" height="7" rx="1.2"/>',
    "distribute-h": '<path d="M3.5 3v18M20.5 3v18"/><rect x="9" y="7" width="6" height="10" rx="1.2"/>',
    "distribute-v": '<path d="M3 3.5h18M3 20.5h18"/><rect x="7" y="9" width="10" height="6" rx="1.2"/>',
    "undo": '<path d="M4.5 8.5H14.5a5.5 5.5 0 0 1 0 11H10"/><path d="M8.5 4.5l-4 4 4 4"/>',
    "redo": '<path d="M19.5 8.5H9.5a5.5 5.5 0 0 0 0 11H14"/><path d="M15.5 4.5l4 4-4 4"/>',
    "discard": '<path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1L3.5 8.5"/><path d="M3.5 3.5v5h5"/><path d="M9.8 9.8l4.4 4.4M14.2 9.8l-4.4 4.4"/>',

    // ---- Chú thích ----
    "highlight": '<path d="M15.5 3.5l5 5-8.3 8.3-5-5z"/><path d="M7.2 11.8l-2.4 4.7 2.7 2.7 4.7-2.4"/><path d="M3 21h18"/>',
    "underline": '<path d="M7 4v6.5a5 5 0 0 0 10 0V4"/><path d="M5 20h14"/>',
    "strikeout": '<path d="M16.5 7.3C15.9 5.8 14.2 5 12 5 9.4 5 7.5 6.3 7.5 8.4c0 1.5 1 2.5 3.1 3.1"/><path d="M4 12h16"/><path d="M15.8 14.6c.5.5.7 1.1.7 1.9 0 2.1-1.9 3.5-4.6 3.5-2.4 0-4.1-.9-4.8-2.6"/>',
    "square": '<rect x="4" y="5" width="16" height="14" rx="2"/>',
    "note": '<path d="M5 3h14a2 2 0 0 1 2 2v9l-7 7H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M14 21v-5a2 2 0 0 1 2-2h5"/><path d="M7 8h10M7 11.5h6"/>',
    // Vẽ (Drawing) + con dấu — features/annot.js
    "pencil": '<path d="M16.3 3.8l3.9 3.9L8.4 19.5 3.5 20.5l1-4.9z"/><path d="M13.8 6.3l3.9 3.9"/>',
    "line": '<path d="M4.5 19.5l15-15"/>',
    "arrow": '<path d="M4.5 19.5l15-15"/><path d="M11 4.5h8.5V13"/>',
    "oval": '<ellipse cx="12" cy="12" rx="8.5" ry="6.5"/>',
    "polygon": '<path d="M12 3.5l8 6-3 9.5H7l-3-9.5z"/>',
    "polyline": '<path d="M3.5 17.5l5-9 5 6 7-9"/>' + dot(3.5, 17.5, 1.3) + dot(8.5, 8.5, 1.3) + dot(13.5, 14.5, 1.3) + dot(20.5, 5.5, 1.3),
    "cloud": '<path d="M7 18.5a4 4 0 0 1-.6-7.95A5.5 5.5 0 0 1 17 8.6a4.5 4.5 0 0 1 .5 8.9z"/>',
    "stamp": '<path d="M9.5 11.5V9.2A3.5 3.5 0 1 1 14.5 9.2v2.3"/><path d="M4 14.5a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v2H4z"/><path d="M5.5 20.5h13"/>',
    "reply": '<path d="M9.5 6L4 11.5 9.5 17"/><path d="M4.5 11.5H14a6 6 0 0 1 6 6v1"/>',
    "pencil-edit": '<path d="M15.5 4.5l4 4L9 19l-5 1 1-5z"/>',
    "eraser": '<path d="M7.5 20.5l-4-4a2 2 0 0 1 0-2.8L13.2 4a2 2 0 0 1 2.8 0l4 4a2 2 0 0 1 0 2.8l-9.7 9.7"/><path d="M7.5 20.5H20.5"/><path d="M8.5 8.7l6.8 6.8"/>',

    // ---- Biểu mẫu ----
    "form-fill": '<rect x="3.5" y="4.5" width="6" height="6" rx="1.3"/><path d="M5 7.6l1.3 1.3L8.2 6.6"/><path d="M12.5 7.5h8"/><rect x="3.5" y="13.5" width="6" height="6" rx="1.3"/><path d="M12.5 16.5h8"/>',
    "field-add": '<rect x="2.5" y="7" width="11" height="10" rx="2"/><path d="M6 10v4"/><path d="M18 8.5v7M14.5 12h7"/>',
    // ---- Form: công cụ tạo trường (features/formx.js) ----
    "field-text": '<rect x="2.5" y="6.5" width="19" height="11" rx="2"/><path d="M6.5 9.5h5M9 9.5v5"/><path d="M15 9v6"/>',
    "field-check": '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M8 12.3l2.7 2.7L16.2 9.5"/>',
    "field-radio": '<circle cx="12" cy="12" r="8.5"/>' + dot(12, 12, 3.6),
    "field-combo": '<rect x="2.5" y="6.5" width="19" height="11" rx="2"/><path d="M15 6.5v11"/><path d="M17 11l1.5 1.6L20 11"/><path d="M6 12h5.5"/>',
    "field-list": '<rect x="3.5" y="3.5" width="17" height="17" rx="2"/><path d="M7 8h10M7 16h7"/>' + bar(6, 10.6, 12, 2.8, 0.8),
    "field-button": '<rect x="2.5" y="7" width="19" height="10" rx="3"/><path d="M8 12h8"/>',
    "field-date": '<rect x="3.5" y="5" width="17" height="15.5" rx="2"/><path d="M3.5 9.5h17M8 3v4M16 3v4"/>' + dot(8, 13.5) + dot(12, 13.5) + dot(16, 13.5) + dot(8, 17) + dot(12, 17),
    "field-sig": '<rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6 14.5c1.2-2.5 2.4-3.5 3.2-2.4.7 1-.2 2.4.8 2.4s1.6-1.8 2.6-1.8.6 1.8 1.6 1.8"/><path d="M15.5 15h3"/>',
    "field-highlight": '<rect x="3" y="6.5" width="18" height="11" rx="2"/>' + bar(5.5, 9, 13, 6, 1),
    "field-edit": '<rect x="3" y="4" width="13" height="9" rx="1.8" stroke-dasharray="2.4 2"/><path d="M12 11l7.5 3.2-3.1 1.2-1.2 3.1z"/>',
    "wand": '<path d="M4 20L15 9"/><path d="M13.5 7.5l3 3"/><path d="M18 3v3M16.5 4.5h3M20.5 10v2.5M19.2 11.2h2.6M9 3v2.5M7.8 4.2h2.5"/>',
    "reset": '<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3"/><path d="M4.5 4.5v4h4"/>',
    "table": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9.5h18M3 14.8h18M9.5 4v16"/>',

    // ---- Chuyển đổi ----
    "ocr": '<path d="M3 7.5V5a2 2 0 0 1 2-2h2.5M16.5 3H19a2 2 0 0 1 2 2v2.5M21 16.5V19a2 2 0 0 1-2 2h-2.5M7.5 21H5a2 2 0 0 1-2-2v-2.5"/><path d="M8 8h8M12 8v8.5"/>',

    // ---- Bảo vệ ----
    "redact": PAGE + '<path d="M8.5 9h3M8.5 17.5h7"/>' + bar(7.5, 11.5, 9, 3.6, 0.9),
    "redact-apply": bar(3, 4, 18, 5.5, 1.3) + '<path d="M7 16l3.5 3.5L17.5 12.5"/>',
    "redact-clear": bar(3, 4, 18, 5.5, 1.3) + '<path d="M8.8 13l6.4 6.4M15.2 13l-6.4 6.4"/>',
    "lock": '<rect x="4.5" y="10.5" width="15" height="10.5" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/><path d="M12 14.5v2.5"/>',
    "unlock": '<rect x="4.5" y="10.5" width="15" height="10.5" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 7.75-1.4"/><path d="M12 14.5v2.5"/>',
    "shield-check": '<path d="M12 3l7.5 2.8v5.7c0 4.4-3.1 8.2-7.5 9.5-4.4-1.3-7.5-5.1-7.5-9.5V5.8z"/><path d="M8.8 12.2l2.3 2.3 4.2-4.4"/>',
    "signature": '<path d="M15.3 3.7l3 3-8.6 8.6-3.8.8.8-3.8z"/><path d="M3 20.5c1.4-1.3 2.6-1.3 3.4-.2.8 1.1 1.8 1.1 3 0 1.2-1.1 2.2-1.1 3 0"/><path d="M15.5 20.5h5.5"/>',
    "id-card": '<rect x="2.5" y="5" width="19" height="14" rx="2"/><circle cx="8.5" cy="10.5" r="2"/><path d="M5.3 15.8a3.3 3.3 0 0 1 6.4 0"/><path d="M14.5 10h4.5M14.5 13.5h3"/>',
    // docsec: che trang / tìm và che / giao diện vùng che / làm sạch / đính kèm / trợ năng
    "redact-page": PAGE + bar(7.5, 10, 9, 8, 1),
    "search-redact": bar(3, 4.5, 11, 4, 1) + '<path d="M3 12.5h6M3 16.5h4"/><circle cx="15.5" cy="14.5" r="3.6"/><path d="M18.2 17.2L21 20"/>',
    "redact-style": bar(3, 3.5, 18, 5, 1.3) + '<path d="M5.5 20.5l3.5-9 3.5 9M6.8 17.3h4.4"/><path d="M15 20.5h6M18 12.5v8"/>',
    "sanitize": '<path d="M15 3l-3.6 8"/><path d="M6.5 11.5h9.5l1.5 9H5z"/><path d="M9 15.5l-.6 5M13 15.5l.2 5"/><path d="M19.5 3.5v3M18 5h3"/>',
    "attach": '<path d="M20 11.5l-8 8a5 5 0 0 1-7.1-7.1l8.4-8.4a3.3 3.3 0 0 1 4.7 4.7l-8.4 8.4a1.6 1.6 0 0 1-2.3-2.3L15 7"/>',
    "attach-add": '<path d="M13 12.8l-4.4 4.4a3.2 3.2 0 0 1-4.5-4.5l6.8-6.8a2.2 2.2 0 0 1 3.1 3.1L7.7 15.3a1.1 1.1 0 0 1-1.6-1.6l5.2-5.2"/><path d="M18 13.5v7M14.5 17h7"/>',
    "a11y": '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="7.3" r="1.3"/><path d="M7.5 9.8h9M12 9.8v4M12 13.8l-2.6 4M12 13.8l2.6 4"/>',
    "a11y-check": '<circle cx="9.5" cy="4.6" r="1.5"/><path d="M3.5 7.8h12M9.5 7.8v5.2M9.5 13l-3.2 7M9.5 13l2.4 4.8"/><path d="M14.5 18l2.2 2.2 4.3-4.7"/>',
    "alt-text": FRAME_B + '<path d="M14.5 16h7M14.5 19.5h5"/>',
    "check-circle": '<circle cx="12" cy="12" r="9"/><path d="M8 12.3l2.7 2.7 5.3-5.6"/>',
    "x-circle": '<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>',
    "help-circle": '<circle cx="12" cy="12" r="9"/><path d="M9.6 9.4a2.5 2.5 0 0 1 4.8.9c0 1.7-2.4 2.2-2.4 3.7"/>' + dot(12, 16.9, 1),

    // ---- Theme / hệ thống ----
    "theme-light": '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>',
    "theme-dark": '<path d="M20 14.6A8 8 0 0 1 9.4 4a8 8 0 1 0 10.6 10.6z"/>',
    "theme-system": '<rect x="3" y="4" width="18" height="12.5" rx="2"/><path d="M8.5 20.5h7M12 16.5v4"/>',
    "keyboard": '<rect x="2.5" y="6" width="19" height="12" rx="2"/>' + dot(6.5, 10) + dot(10, 10) + dot(14, 10) + dot(17.5, 10) + dot(6.5, 14) + dot(17.5, 14) + '<path d="M9.5 14h5"/>',
    "info": '<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5"/>' + dot(12, 7.8, 1),
    "clock": '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.2 2"/>',

    // ---- Bookmark / liên kết ----
    "bookmark": '<path d="M7 3.5h10a1 1 0 0 1 1 1V20.5l-6-4-6 4V4.5a1 1 0 0 1 1-1z"/>',
    "bookmark-add": '<path d="M13 3.5H7a1 1 0 0 0-1 1V20.5l6-4 6 4V12"/><path d="M18.5 3v6M15.5 6h6"/>',
    "bookmark-child": '<path d="M4 4v6a2 2 0 0 0 2 2h4"/><path d="M13 8.5h6a1 1 0 0 1 1 1v11l-4-2.7-4 2.7v-11a1 1 0 0 1 1-1z"/>',
    "bookmark-auto": '<path d="M7 3.5h6M6 4.5V20.5l6-4 6 4V13"/>' + '<path d="M18 2.5l.9 2.1 2.1.9-2.1.9-.9 2.1-.9-2.1-2.1-.9 2.1-.9z"/>',
    "rename": '<path d="M4 20h4L19.5 8.5a2.1 2.1 0 0 0-3-3L5 17v3z"/><path d="M14.5 7.5l3 3"/><path d="M13 20h7"/>',
    "outdent": '<path d="M11 6h9M11 12h9M4 18h16"/><path d="M8 9.5L4.5 12 8 14.5"/>',
    "indent": '<path d="M11 6h9M11 12h9M4 18h16"/><path d="M4.5 9.5L8 12l-3.5 2.5"/>',
    "link": '<path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1.2 1.2"/><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1.2-1.2"/>',
    "more": dot(6, 12, 1.4) + dot(12, 12, 1.4) + dot(18, 12, 1.4),
    "expand-all": '<path d="M7 9l5-5 5 5M7 15l5 5 5-5"/>',
    "collapse-all": '<path d="M7 4l5 5 5-5M7 20l5-5 5 5"/>',
    // ---- AI Assistant / Đọc to ----
    "sparkle": '<path d="M11 3.5l1.7 4.6a2 2 0 0 0 1.2 1.2l4.6 1.7-4.6 1.7a2 2 0 0 0-1.2 1.2L11 18.5l-1.7-4.6a2 2 0 0 0-1.2-1.2L3.5 11l4.6-1.7a2 2 0 0 0 1.2-1.2z"/><path d="M18.5 15v5M16 17.5h5"/>',
    "summary": '<path d="M4 5.5h16M4 10h16M4 14.5h10M4 19h6.5"/>',
    "key-points": dot(5, 6.5, 1.3) + dot(5, 12, 1.3) + dot(5, 17.5, 1.3) + '<path d="M9 6.5h11M9 12h11M9 17.5h7"/>',
    "translate": '<path d="M3.5 5.5h9M8 3.5v2M5.5 5.5c.8 3 2.9 5.6 5.8 7M10.5 5.5c-.8 3.6-3.2 6.5-6.5 8"/><path d="M12.5 20.5l3.8-9 3.7 9M13.8 17.5h5"/>',
    "explain": '<circle cx="12" cy="12" r="9"/><path d="M9.6 9.4a2.5 2.5 0 1 1 3.6 2.3c-.7.3-1.2 1-1.2 1.8v.5"/>' + dot(12, 17, 1),
    "spellcheck": '<path d="M3.5 15l3.5-9.5 3.5 9.5M4.8 11.5h4.4"/><path d="M13 5.5h3.2a2.2 2.2 0 0 1 0 4.4H13zM13 9.9h3.8a2.3 2.3 0 0 1 0 4.6H13z"/><path d="M12 18.5l2.5 2.5 6-6"/>',
    "speaker": '<path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/>',
    "gear": '<circle cx="12" cy="12" r="3"/><path d="M19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.5-2-3.4-2.3.9a7.5 7.5 0 0 0-2.6-1.5L14 2.5h-4l-.5 2.5a7.5 7.5 0 0 0-2.6 1.5l-2.3-.9-2 3.4 2 1.5a7.6 7.6 0 0 0 0 3l-2 1.5 2 3.4 2.3-.9a7.5 7.5 0 0 0 2.6 1.5l.5 2.5h4l.5-2.5a7.5 7.5 0 0 0 2.6-1.5l2.3.9 2-3.4z"/>',
    "chat-new": '<path d="M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-7l-4.5 3.5V17H6a2 2 0 0 1-2-2z"/><path d="M12 7.5v6M9 10.5h6"/>',
    "paperclip": '<path d="M20 11.5l-7.8 7.8a5 5 0 0 1-7.1-7.1l8.1-8.1a3.3 3.3 0 0 1 4.7 4.7l-8.1 8.1a1.7 1.7 0 0 1-2.4-2.4l7.4-7.4"/>',
    "send": '<path d="M4.5 12.5L20 4.5l-4.5 15.5-3.8-6.2z"/><path d="M11.7 13.8L20 4.5"/>',
    "stop": '<rect x="6.5" y="6.5" width="11" height="11" rx="2"/>',
    "play": '<path d="M8 5.5v13l10-6.5z"/>',
    "pause": '<path d="M8.5 5.5v13M15.5 5.5v13"/>',
    // ---- Tạo PDF / Chuyển đổi (features/create.js) ----
    "file-plus": PAGE_B + '<path d="M17 13.5v7M13.5 17h7"/>',
    "file-blank": PAGE,
    "file-excel": PAGE + '<path d="M8.5 11.5l5 6M13.5 11.5l-5 6"/>',
    "file-ppt": PAGE + '<path d="M9 18v-6.5h2.6a2 2 0 0 1 0 4H9"/>',
    "file-code": PAGE + '<path d="M10 12l-2 2.2 2 2.2M14 12l2 2.2-2 2.2"/>',
    "globe": '<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3c2.5 2.6 3.7 5.6 3.7 9s-1.2 6.4-3.7 9c-2.5-2.6-3.7-5.6-3.7-9S9.5 5.6 12 3z"/>',
    "scanner": '<path d="M4 14h16a1 1 0 0 1 1 1v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3a1 1 0 0 1 1-1z"/><path d="M3.5 14L18 5.5"/><path d="M7 17.5h2"/>' + dot(17, 17.5),
    "clipboard": '<rect x="5" y="4.5" width="14" height="16.5" rx="2"/><path d="M9 4.5V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v.5"/><path d="M9 3.5h6v2.5H9z"/><path d="M8.5 11h7M8.5 14.5h7M8.5 18h4"/>',
    "ocr-image": '<path d="M3 7.5V5a2 2 0 0 1 2-2h2.5M16.5 3H19a2 2 0 0 1 2 2v2.5M21 16.5V19a2 2 0 0 1-2 2h-2.5M7.5 21H5a2 2 0 0 1-2-2v-2.5"/><circle cx="9.5" cy="9.5" r="1.5"/><path d="M6.5 17l3.5-3.6 2.5 2.5 1.7-1.7 3.3 2.8"/>',
    "arrow-up": '<path d="M12 19V5M6.5 10.5L12 5l5.5 5.5"/>',
    "arrow-down": '<path d="M12 5v14M6.5 13.5L12 19l5.5-5.5"/>',
  };
})();
function icon(name) {
  const body = ICONS[name] || '<rect x="5" y="5" width="14" height="14" rx="3"/>';
  return `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}
function applyIcons(root) {
  (root || document).querySelectorAll("i[data-icon]").forEach((el) => {
    if (el.dataset.iconDone === el.dataset.icon) return;
    el.innerHTML = icon(el.dataset.icon);
    el.dataset.iconDone = el.dataset.icon;
  });
}
