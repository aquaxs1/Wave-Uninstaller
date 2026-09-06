/* Shared site behaviour: theme, mobile nav, footer year.
   No framework, no build step — the same rule the product follows. */

"use strict";

/* ------------------------------------------------------------------ theme */

const THEME_KEY = "wave-site-theme";

function resolveTheme(choice) {
  if (choice === "light" || choice === "dark") return choice;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function applyTheme(choice) {
  const resolved = resolveTheme(choice);
  document.documentElement.dataset.theme = resolved;
  document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
    button.setAttribute(
      "aria-label",
      resolved === "dark" ? "Switch to light theme" : "Switch to dark theme",
    );
    const use = button.querySelector("use");
    if (use) use.setAttribute("href", resolved === "dark" ? "#i-sun" : "#i-moon");
  });
  try {
    localStorage.setItem(THEME_KEY, choice);
  } catch {
    /* private mode */
  }
}

function initTheme() {
  let stored = "system";
  try {
    stored = localStorage.getItem(THEME_KEY) || "system";
  } catch {
    /* private mode */
  }
  applyTheme(stored);

  document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
    button.addEventListener("click", () => {
      applyTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
    });
  });
}

/* -------------------------------------------------------------- mobile nav */

function initNav() {
  const toggle = document.querySelector("[data-nav-toggle]");
  const nav = document.getElementById("site-nav");
  if (!toggle || !nav) return;

  const isMobile = () => matchMedia("(max-width: 900px)").matches;
  const setOpen = (open) => {
    nav.hidden = !open;
    toggle.setAttribute("aria-expanded", String(open));
  };

  // On a wide screen the nav is always visible, so the toggle's state has to
  // be re-derived on resize rather than remembered.
  const sync = () => setOpen(!isMobile());
  sync();
  addEventListener("resize", sync);

  toggle.addEventListener("click", () => setOpen(nav.hidden));
  nav.addEventListener("click", (event) => {
    if (event.target.tagName === "A" && isMobile()) setOpen(false);
  });
  addEventListener("keydown", (event) => {
    if (event.key === "Escape" && isMobile()) setOpen(false);
  });
}

/* -------------------------------------------------------------------- year */

function initYear() {
  const year = String(new Date().getFullYear());
  document.querySelectorAll("[data-year]").forEach((node) => {
    node.textContent = year;
  });
}

initTheme();
initNav();
initYear();
