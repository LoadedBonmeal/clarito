import i18n from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { initReactI18next } from "react-i18next";

import en from "@/locales/en.json";
import ro from "@/locales/ro.json";

/**
 * Per-namespace locale files: src/locales/{ro,en}/<ns>.json, each exporting a
 * single top-level namespace object (e.g. { "invoices": { ... } }). They are
 * merged into the translation resource at init — one file per domain, so pages
 * and translators never collide on a shared JSON.
 */
const roModules = import.meta.glob("../locales/ro/*.json", { eager: true }) as Record<string, { default: Record<string, unknown> }>;
const enModules = import.meta.glob("../locales/en/*.json", { eager: true }) as Record<string, { default: Record<string, unknown> }>;

const merge = (base: Record<string, unknown>, modules: Record<string, { default: Record<string, unknown> }>) => {
  const out: Record<string, unknown> = { ...base };
  for (const mod of Object.values(modules)) Object.assign(out, mod.default);
  return out;
};

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      ro: { translation: merge(ro as Record<string, unknown>, roModules) },
      en: { translation: merge(en as Record<string, unknown>, enModules) },
    },
    lng: "ro",
    fallbackLng: "ro",
    supportedLngs: ["ro", "en"],
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: ["localStorage"],
      caches: ["localStorage"],
      lookupLocalStorage: "rofactura.lang",
    },
  });

export default i18n;
