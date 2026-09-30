// i18n: từ điển VI/EN phẳng, key có ngữ cảnh. Đổi ngôn ngữ áp dụng tức thì.
(function () {
  const dict = { vi: {}, en: {} };
  let lang = null;
  try { lang = localStorage.getItem("ff.lang"); } catch (_) {}
  if (lang !== "vi" && lang !== "en") {
    lang = /^vi\b/i.test(navigator.language || "") ? "vi" : "en";
  }
  function fmt(s, vars) {
    return vars ? s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : s;
  }
  window.t = function (key, vars) {
    const s = dict[lang][key] ?? dict.vi[key] ?? key;
    return fmt(s, vars);
  };
  window.applyI18n = function (root) {
    const r = root || document;
    r.querySelectorAll("[data-i18n]").forEach((el) => { el.textContent = t(el.dataset.i18n); });
    r.querySelectorAll("[data-i18n-title]").forEach((el) => { el.title = t(el.dataset.i18nTitle); });
    r.querySelectorAll("[data-i18n-placeholder]").forEach((el) => { el.placeholder = t(el.dataset.i18nPlaceholder); });
    r.querySelectorAll("[data-i18n-aria]").forEach((el) => { el.setAttribute("aria-label", t(el.dataset.i18nAria)); });
  };
  window.I18N = {
    dict,
    get lang() { return lang; },
    add(l, obj) { Object.assign(dict[l], obj); },
    setLang(l) {
      if (l !== "vi" && l !== "en") return;
      lang = l;
      try { localStorage.setItem("ff.lang", l); } catch (_) {}
      document.documentElement.lang = l;
      applyI18n(document);
      document.dispatchEvent(new CustomEvent("langchange", { detail: l }));
    },
  };
  document.documentElement.lang = lang;
})();
