import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type Appearance = "system" | "light" | "dark";
const key = "opendots.appearance";
export function readAppearance(): Appearance {
  try {
    const value = localStorage.getItem(key);
    if (value === "light" || value === "dark") return value;
  } catch {
    /* System appearance remains available if storage is unavailable. */
  }
  return "system";
}
export function applyAppearance(preference: Appearance) {
  const resolved =
    preference === "system"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : preference;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.style.colorScheme = resolved;
  if (isTauri()) {
    void getCurrentWindow()
      .setTheme(preference === "system" ? null : preference)
      .catch((error) =>
        console.error("Could not update native window appearance", error),
      );
  }
}
export function saveAppearance(preference: Appearance) {
  try {
    localStorage.setItem(key, preference);
  } catch {
    /* Apply for this session. */
  }
  applyAppearance(preference);
}
export function initializeAppearance() {
  applyAppearance(readAppearance());
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const update = () => applyAppearance(readAppearance());
  query.addEventListener("change", update);
  return () => query.removeEventListener("change", update);
}
