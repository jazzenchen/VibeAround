import { createContext, useContext } from "react";

const STORAGE_KEY = "vibearound-theme";

export type Theme = "light" | "dark";

export const ThemeContext = createContext<Theme>("light");

export function useTheme(): Theme {
  return useContext(ThemeContext);
}

function getSystemTheme(): Theme {
  if (typeof window === "undefined" || !window.matchMedia) return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function getStoredTheme(): Theme | null {
  if (typeof window === "undefined") return null;
  const v = localStorage.getItem(STORAGE_KEY);
  if (v === "light" || v === "dark") return v;
  return null;
}

export function setStoredTheme(theme: Theme): void {
  if (typeof window === "undefined") return;
  localStorage.setItem(STORAGE_KEY, theme);
}

/** Resolved theme: stored preference or system. */
export function getResolvedTheme(): Theme {
  return getStoredTheme() ?? getSystemTheme();
}

export function applyTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  if (theme === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
}

/** Run once on app load to apply stored/system theme (avoids flash). */
export function initTheme(): Theme {
  const theme = getResolvedTheme();
  applyTheme(theme);
  return theme;
}

export function toggleTheme(current: Theme): Theme {
  const next: Theme = current === "light" ? "dark" : "light";
  setStoredTheme(next);
  applyTheme(next);
  return next;
}
