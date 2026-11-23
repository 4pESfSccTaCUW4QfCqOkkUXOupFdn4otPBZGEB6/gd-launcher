/**
 * Colour themes. Each one is a ground plus an accent; everything else in the UI is
 * a grey derived from the ground, so no theme can end up fighting itself. The
 * actual values live in `styles.css` under `[data-theme="…"]` - this list only
 * drives the picker and the two swatch colours.
 */
export interface Theme {
  id: string;
  name: string;
  ground: string;
  accent: string;
  light?: boolean;
}

export const THEMES: Theme[] = [
  { id: "amber", name: "amber", ground: "#000000", accent: "#ffb000" },
  { id: "phosphor", name: "phosphor", ground: "#000000", accent: "#3dff9a" },
  { id: "ice", name: "ice", ground: "#04070a", accent: "#6fd3ff" },
  { id: "violet", name: "violet", ground: "#07040a", accent: "#c08bff" },
  { id: "ember", name: "ember", ground: "#0a0503", accent: "#ff7a45" },
  { id: "rose", name: "rose", ground: "#0a0407", accent: "#ff6aa8" },
  { id: "slate", name: "slate", ground: "#0b0d10", accent: "#9fb8d4" },
  { id: "paper", name: "paper", ground: "#f5f2ea", accent: "#a4540a", light: true },
];

export const DEFAULT_THEME = "amber";
const KEY = "gdlauncher.theme";

export function isTheme(id: string): boolean {
  return THEMES.some((t) => t.id === id);
}

export function storedTheme(): string {
  try {
    const saved = window.localStorage.getItem(KEY);
    if (saved && isTheme(saved)) return saved;
  } catch {
    // Private mode or a locked store - fall through to the default.
  }
  return DEFAULT_THEME;
}

export function applyTheme(id: string): void {
  const next = isTheme(id) ? id : DEFAULT_THEME;
  document.documentElement.dataset.theme = next;
  try {
    window.localStorage.setItem(KEY, next);
  } catch {
    // Not fatal: the theme still applies for this session.
  }
}
