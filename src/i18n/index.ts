// Interface texts. Each language is a dictionary with the same shape as fr.ts,
// so a missing or misnamed text is a type error. Texts that depend on values
// are functions, which also handles plurals and word order per language.

import { ref } from 'vue';
import fr from './fr';
import en from './en';

export type Messages = typeof fr;
export type Locale = 'fr' | 'en';
/// What the settings store: a language, or the system's.
export type LanguageSetting = Locale | 'system';

const dictionaries: Record<Locale, Messages> = { fr, en };

function systemLocale(): Locale {
  return navigator.language.toLowerCase().startsWith('fr') ? 'fr' : 'en';
}

export function resolveLocale(setting: string): Locale {
  return setting === 'fr' || setting === 'en' ? setting : systemLocale();
}

export const locale = ref<Locale>(systemLocale());

/// The current dictionary. Reading through it in a template or a computed
/// makes it follow language changes.
export const t: Messages = new Proxy({} as Messages, {
  get: (_target, key) => dictionaries[locale.value][key as keyof Messages],
});

export function setLanguage(setting: string) {
  locale.value = resolveLocale(setting);
  document.documentElement.lang = locale.value;
}

/// For Intl date and number formatting.
export function intlLocale() {
  return locale.value === 'fr' ? 'fr-FR' : 'en-US';
}
