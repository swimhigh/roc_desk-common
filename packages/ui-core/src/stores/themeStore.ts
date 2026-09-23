import { create } from "zustand";

export type Theme = "dark" | "light";

const STORAGE_KEY = "roc_desk-theme";

function applyTheme(theme: Theme) {
  document.documentElement.setAttribute("data-theme", theme);
}

function loadInitialTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "light" ? "light" : "dark"; // 深色优先，无存储偏好时默认深色
}

interface ThemeState {
  theme: Theme;
  toggle: () => void;
  setTheme: (theme: Theme) => void;
}

/** 主题切换——和宿主 roc_desk 的深色优先约定保持一致。*/
export const useThemeStore = create<ThemeState>((set, get) => {
  const initial = loadInitialTheme();
  applyTheme(initial);
  return {
    theme: initial,
    toggle: () => get().setTheme(get().theme === "dark" ? "light" : "dark"),
    setTheme: (theme) => {
      applyTheme(theme);
      localStorage.setItem(STORAGE_KEY, theme);
      set({ theme });
    },
  };
});
