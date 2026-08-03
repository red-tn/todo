// theme.js — light / dark / system theme, persisted in localStorage.
const KEY = "todo-theme";
const mq = window.matchMedia("(prefers-color-scheme: dark)");

/** Stored preference: "light" | "dark" | "system" (default). */
export function getTheme() {
  return localStorage.getItem(KEY) || "system";
}

function resolve(pref) {
  if (pref === "system") return mq.matches ? "dark" : "light";
  return pref;
}

function apply(pref) {
  document.documentElement.setAttribute("data-theme", resolve(pref));
}

export function setTheme(pref) {
  localStorage.setItem(KEY, pref);
  apply(pref);
}

/** Apply the saved theme and keep "system" in sync with the OS. */
export function initTheme() {
  apply(getTheme());
  mq.addEventListener("change", () => {
    if (getTheme() === "system") apply("system");
  });
}
