// Applies the saved theme before the first paint (dark when there is none); see `src/sidepanel/theme.ts`. A file of its own, not an
// inline script: an extension page's default content security policy refuses inline scripts.
try {
  var saved = localStorage.getItem("warden-theme");
  if (saved === "light" || saved === "dark") document.documentElement.dataset.theme = saved;
} catch (e) {}
