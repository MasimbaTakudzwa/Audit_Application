import { get, writable } from "svelte/store";

export type Theme = "light" | "dark";

function initial(): Theme {
  if (typeof window === "undefined") return "light";
  const stored = localStorage.getItem("theme") as Theme | null;
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export const theme = writable<Theme>(initial());

theme.subscribe((value) => {
  if (typeof document === "undefined") return;
  document.documentElement.classList.toggle("dark", value === "dark");
  try {
    localStorage.setItem("theme", value);
  } catch {
    /* storage disabled */
  }
});

export function toggleTheme() {
  theme.set(get(theme) === "dark" ? "light" : "dark");
}
