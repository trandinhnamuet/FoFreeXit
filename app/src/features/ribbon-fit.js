// Ribbon tự co gọn khi cửa sổ hẹp (như Foxit): panel đang hiện mà tràn ngang
// → bậc 1 thu hẹp nút + chữ nhỏ; vẫn tràn → bậc 2 chỉ còn icon (nhãn vẫn ở
// tooltip). Không đổi cấu trúc nút nên mọi id/sự kiện giữ nguyên.
(function () {
  const ribbon = $("ribbon");
  if (!ribbon) return;
  const LEVELS = ["fit-1", "fit-2"];
  let raf = 0;

  function overflowing(p) { return p.scrollWidth > p.clientWidth + 1; }

  function fit() {
    raf = 0;
    const p = ribbon.querySelector(".rpanel.on");
    if (!p) return;
    p.classList.remove(...LEVELS);
    for (const lv of LEVELS) {
      if (!overflowing(p)) break;
      p.classList.add(lv);
    }
  }
  function schedule() { if (!raf) raf = requestAnimationFrame(fit); }

  window.addEventListener("resize", schedule);
  document.addEventListener("langchange", schedule);
  new MutationObserver(schedule).observe(ribbon, { subtree: true, attributes: true, attributeFilter: ["class"], childList: true });
  if (window.ResizeObserver) new ResizeObserver(schedule).observe(ribbon);
  schedule();
})();
