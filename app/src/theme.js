// Theme: light | dark | system (theo Windows, đổi trực tiếp khi hệ thống đổi).
(function () {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  let pref = "system";
  try { pref = localStorage.getItem("ff.theme") || "system"; } catch (_) {}
  if (!["light", "dark", "system"].includes(pref)) pref = "system";
  function paint() {
    const eff = pref === "system" ? (mq.matches ? "dark" : "light") : pref;
    document.documentElement.dataset.theme = eff;
    document.documentElement.style.colorScheme = eff;
    document.dispatchEvent(new CustomEvent("themechange", { detail: { pref, effective: eff } }));
    return eff;
  }
  function apply() {
    const eff = paint();
    // Khung/thanh tiêu đề gốc của Windows (nếu có) theo đúng theme của app. "system" → null
    // (theo hệ thống): truyền giá trị cụ thể sẽ ghim prefers-color-scheme của WebView2 và
    // listener `mq` thôi nhận thay đổi của Windows. Bỏ ghim xong thì `mq` mới phản ánh
    // đúng hệ thống → vẽ lại một lần.
    try {
      const w = window.__TAURI__ && window.__TAURI__.window;
      if (w) {
        w.getCurrentWindow().setTheme(pref === "system" ? null : eff)
          .then(() => { if (pref === "system") paint(); })
          .catch(() => {});
      }
    } catch (_) {}
  }
  mq.addEventListener("change", () => { if (pref === "system") paint(); });
  window.Theme = {
    get pref() { return pref; },
    get effective() { return document.documentElement.dataset.theme; },
    set(p) {
      if (!["light", "dark", "system"].includes(p)) return;
      pref = p;
      try { localStorage.setItem("ff.theme", p); } catch (_) {}
      apply();
    },
  };
  apply();
})();
