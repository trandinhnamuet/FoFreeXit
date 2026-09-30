# app/src/features

Mỗi tính năng lớn một tệp JS riêng (script thường, nạp sau `main.js` trong
`index.html`), dùng các hàm/biến toàn cục của `main.js` (`invoke`, `$`, `state`,
`t`, `openModal`, `closeModal`, `loadDocument`, `shortName`, `parsePageRange`,
`applyIcons`...). Từ điển i18n tương ứng: `app/src/i18n/part-<tên>.js`.
`scripts/check-i18n.mjs` và `scripts/check-ids.mjs` quét cả thư mục này.
