// Bộ icon SVG line 24px (stroke currentColor). <i data-icon="name"></i> → SVG.
const ICONS = {
  "win-min": '<path d="M5 12h14"/>',
  "win-max": '<rect x="5" y="5" width="14" height="14" rx="1.5"/>',
  "win-restore": '<rect x="5" y="8" width="11" height="11" rx="1.5"/><path d="M8 8V6.5A1.5 1.5 0 0 1 9.5 5H17.5A1.5 1.5 0 0 1 19 6.5V14.5A1.5 1.5 0 0 1 17.5 16H16"/>',
  "win-close": '<path d="M6 6l12 12M18 6L6 18"/>',
  "menu": '<path d="M4 7h16M4 12h16M4 17h16"/>',
  "chevron-up": '<path d="M6 15l6-6 6 6"/>',
  "chevron-down": '<path d="M6 9l6 6 6-6"/>',
  "chevron-left": '<path d="M15 6l-6 6 6 6"/>',
  "chevron-right": '<path d="M9 6l6 6-6 6"/>',
  "save": '<path d="M5 4h11l3 3v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4z"/><path d="M8 4v5h7V4M8 20v-6h8v6"/>',
  "folder-open": '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v1"/><path d="M3 7v11a1 1 0 0 0 1 1h13.5a1 1 0 0 0 .96-.72L21 11H7.5a1 1 0 0 0-.96.72L4 19"/>',
  "search": '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
};
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
