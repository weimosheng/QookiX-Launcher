import { createI18n } from "vue-i18n";
import zhCN from "./locales/zh-CN";
import enUS from "./locales/en-US";

export type AppLocale = "zh-CN" | "en-US";

export const SUPPORTED_LOCALES: { value: AppLocale; label: string }[] = [
  { value: "zh-CN", label: "简体中文" },
  { value: "en-US", label: "English" },
];

export const DEFAULT_LOCALE: AppLocale = "zh-CN";

const i18n = createI18n({
  legacy: false,
  locale: DEFAULT_LOCALE,
  fallbackLocale: DEFAULT_LOCALE,
  messages: {
    "zh-CN": zhCN,
    "en-US": enUS,
  },
});

export default i18n;
