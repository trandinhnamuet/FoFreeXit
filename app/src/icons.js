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
    "undo": '<path d="M4.5 8.5H14.5a5.5 5.5 0 0 1 0 11H10"/><path d="M8.5 4.5l-4 4 4 4"/>',
    "redo": '<path d="M19.5 8.5H9.5a5.5 5.5 0 0 0 0 11H14"/><path d="M15.5 4.5l4 4-4 4"/>',
    "discard": '<path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1L3.5 8.5"/><path d="M3.5 3.5v5h5"/><path d="M9.8 9.8l4.4 4.4M14.2 9.8l-4.4 4.4"/>',

    // ---- Chú thích ----
    "highlight": '<path d="M15.5 3.5l5 5-8.3 8.3-5-5z"/><path d="M7.2 11.8l-2.4 4.7 2.7 2.7 4.7-2.4"/><path d="M3 21h18"/>',
    "underline": '<path d="M7 4v6.5a5 5 0 0 0 10 0V4"/><path d="M5 20h14"/>',
    "strikeout": '<path d="M16.5 7.3C15.9 5.8 14.2 5 12 5 9.4 5 7.5 6.3 7.5 8.4c0 1.5 1 2.5 3.1 3.1"/><path d="M4 12h16"/><path d="M15.8 14.6c.5.5.7 1.1.7 1.9 0 2.1-1.9 3.5-4.6 3.5-2.4 0-4.1-.9-4.8-2.6"/>',
    "square": '<rect x="4" y="5" width="16" height="14" rx="2"/>',
    "note": '<path d="M5 3h14a2 2 0 0 1 2 2v9l-7 7H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M14 21v-5a2 2 0 0 1 2-2h5"/><path d="M7 8h10M7 11.5h6"/>',
    "eraser": '<path d="M7.5 20.5l-4-4a2 2 0 0 1 0-2.8L13.2 4a2 2 0 0 1 2.8 0l4 4a2 2 0 0 1 0 2.8l-9.7 9.7"/><path d="M7.5 20.5H20.5"/><path d="M8.5 8.7l6.8 6.8"/>',

    // ---- Biểu mẫu ----
    "form-fill": '<rect x="3.5" y="4.5" width="6" height="6" rx="1.3"/><path d="M5 7.6l1.3 1.3L8.2 6.6"/><path d="M12.5 7.5h8"/><rect x="3.5" y="13.5" width="6" height="6" rx="1.3"/><path d="M12.5 16.5h8"/>',
    "field-add": '<rect x="2.5" y="7" width="11" height="10" rx="2"/><path d="M6 10v4"/><path d="M18 8.5v7M14.5 12h7"/>',
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

    // ---- Theme / hệ thống ----
    "theme-light": '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>',
    "theme-dark": '<path d="M20 14.6A8 8 0 0 1 9.4 4a8 8 0 1 0 10.6 10.6z"/>',
    "theme-system": '<rect x="3" y="4" width="18" height="12.5" rx="2"/><path d="M8.5 20.5h7M12 16.5v4"/>',
    "keyboard": '<rect x="2.5" y="6" width="19" height="12" rx="2"/>' + dot(6.5, 10) + dot(10, 10) + dot(14, 10) + dot(17.5, 10) + dot(6.5, 14) + dot(17.5, 14) + '<path d="M9.5 14h5"/>',
    "info": '<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5"/>' + dot(12, 7.8, 1),
    "clock": '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.2 2"/>',
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
