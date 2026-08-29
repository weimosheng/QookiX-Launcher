import type { GlobalThemeOverrides } from "naive-ui";

/** Warm amber accent derived from the app icon (RGB ~224,160,80). */
export const DEFAULT_ACCENT = "#e89a4b";

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export function hexToRgb(hex: string): Rgb {
  let h = (hex || "").trim().replace("#", "");
  if (h.length === 3) {
    h = h
      .split("")
      .map((c) => c + c)
      .join("");
  }
  const num = parseInt(h, 16);
  if (Number.isNaN(num) || h.length !== 6) {
    return { r: 232, g: 154, b: 75 };
  }
  return { r: (num >> 16) & 255, g: (num >> 8) & 255, b: num & 255 };
}

/** Darken a hex color by the given ratio (0-1). */
export function darken(hex: string, amount = 0.14): string {
  const { r, g, b } = hexToRgb(hex);
  const f = (v: number) => Math.round(v * (1 - amount));
  return rgbToHex(f(r), f(g), f(b));
}

/** Lighten a hex color toward white by the given ratio (0-1). */
export function lighten(hex: string, amount = 0.12): string {
  const { r, g, b } = hexToRgb(hex);
  const f = (v: number) => Math.round(v + (255 - v) * amount);
  return rgbToHex(f(r), f(g), f(b));
}

export function rgbToHex(r: number, g: number, b: number): string {
  const f = (v: number) => Math.max(0, Math.min(255, v)).toString(16).padStart(2, "0");
  return `#${f(r)}${f(g)}${f(b)}`;
}

export function rgba(hex: string, alpha: number): string {
  const { r, g, b } = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

export const ACCENT = DEFAULT_ACCENT;
export const ACCENT_DEEP = darken(DEFAULT_ACCENT);
export const ACCENT_SOFT = rgba(DEFAULT_ACCENT, 0.14);

export function buildDarkOverrides(accent: string): GlobalThemeOverrides {
  const primary = accent;
  const hover = lighten(accent, 0.12);
  const pressed = darken(accent, 0.14);
  return {
    common: {
      primaryColor: primary,
      primaryColorHover: hover,
      primaryColorPressed: pressed,
      primaryColorSuppl: primary,
      borderRadius: "10px",
      borderRadiusSmall: "8px",
      fontSize: "14px",
      textColorBase: "#e8e8ee",
      textColor1: "#f2f3f7",
      textColor2: "#d4d5dd",
      textColor3: "#9b9daa",
      bodyColor: "transparent",
      cardColor: "rgba(255,255,255,0.045)",
      modalColor: "#171a22",
      popoverColor: "#1a1d26",
      inputColor: "rgba(255,255,255,0.06)",
      borderColor: "rgba(255,255,255,0.09)",
      dividerColor: "rgba(255,255,255,0.08)",
      hoverColor: "rgba(255,255,255,0.06)",
      successColor: "#4ec9a0",
      errorColor: "#e5534b",
      warningColor: "#e0a030",
      infoColor: "#5aa2f0",
    },
    Button: {
      borderRadiusMedium: "10px",
      fontWeight: "600",
    },
    Card: {
      borderRadius: "14px",
    },
    Input: {
      borderRadius: "10px",
    },
    Select: {
      peers: {
        InternalSelection: { borderRadius: "10px" },
      },
    },
    Dialog: {
      borderRadius: "16px",
    },
    Modal: {
      borderRadius: "16px",
    },
    Tabs: {
      tabBorderRadius: "8px",
    },
  };
}

export function buildLightOverrides(accent: string): GlobalThemeOverrides {
  const primary = lighten(accent, 0.12);
  const hover = lighten(accent, 0.2);
  const pressed = darken(accent, 0.12);
  return {
    common: {
      primaryColor: primary,
      primaryColorHover: hover,
      primaryColorPressed: pressed,
      primaryColorSuppl: primary,
      borderRadius: "10px",
      borderRadiusSmall: "8px",
      fontSize: "14px",
      textColorBase: "#3a3d48",
      textColor1: "#1a1d24",
      textColor2: "#4a4d58",
      textColor3: "#8b8e9c",
      bodyColor: "transparent",
      cardColor: "rgba(255,255,255,0.6)",
      modalColor: "#ffffff",
      popoverColor: "#ffffff",
      inputColor: "rgba(0,0,0,0.03)",
      borderColor: "rgba(0,0,0,0.1)",
      dividerColor: "rgba(0,0,0,0.08)",
      hoverColor: "rgba(0,0,0,0.04)",
      successColor: "#2d9b6f",
      errorColor: "#d63d35",
      warningColor: "#d97f33",
      infoColor: "#3b82c4",
    },
    Button: {
      borderRadiusMedium: "10px",
      fontWeight: "600",
    },
    Card: {
      borderRadius: "14px",
    },
    Input: {
      borderRadius: "10px",
    },
    Select: {
      peers: {
        InternalSelection: { borderRadius: "10px" },
      },
    },
    Dialog: {
      borderRadius: "16px",
    },
    Modal: {
      borderRadius: "16px",
    },
    Tabs: {
      tabBorderRadius: "8px",
    },
  };
}

/** Accent alpha levels used across the stylesheets, exposed as `--accent-01` ... `--accent-60`. */
export const ACCENT_ALPHAS = [
  0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.1, 0.12, 0.14, 0.16, 0.2, 0.22,
  0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.6,
];

export function accentAlphaVar(alpha: number): string {
  const digits = String(Math.round(alpha * 100)).padStart(2, "0");
  return `--accent-${digits}`;
}
