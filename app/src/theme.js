// Theme: light | dark | system (theo Windows, đổi trực tiếp khi hệ thống đổi).
(function () {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  let pref = "system";
  try { pref = localStorage.getItem("ff.theme") || "system"; } catch (_) {}
  if (!["light", "dark", "system"].includes(pref)) pref = "system";
  function apply() {
    const eff = pref === "system" ? (mq.matches ? "dark" : "light") : pref;
    document.documentElement.dataset.theme = eff;
    document.documentElement.style.colorScheme = eff;
    document.dispatchEvent(new CustomEvent("themechange", { detail: { pref, effective: eff } }));
  }
  mq.addEventListener("change", () => { if (pref === "system") apply(); });
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
