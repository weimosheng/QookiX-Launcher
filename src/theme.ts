import type { GlobalThemeOverrides } from "naive-ui";

/** Warm amber accent derived from the app icon (RGB ~224,160,80). */
export const ACCENT = "#e89a4b";
export const ACCENT_DEEP = "#d97f33";
export const ACCENT_SOFT = "rgba(232, 154, 75, 0.14)";

export const darkThemeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: ACCENT,
    primaryColorHover: "#f0ab63",
    primaryColorPressed: ACCENT_DEEP,
    primaryColorSuppl: ACCENT,
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

export const lightThemeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: ACCENT_DEEP,
    primaryColorHover: ACCENT,
    primaryColorPressed: "#c06a22",
    borderRadius: "10px",
    bodyColor: "transparent",
  },
};
