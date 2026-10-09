import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  detectLocale,
  numberLocale,
  persistLocale,
  translate,
  type Locale,
  type MessageKey,
} from './locales'

type TFn = (key: MessageKey, vars?: Record<string, string | number>) => string

type I18nValue = {
  locale: Locale
  setLocale: (locale: Locale) => void
  t: TFn
  formatNumber: (value: number) => string
}

const I18nContext = createContext<I18nValue | null>(null)

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => detectLocale())

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next)
    persistLocale(next)
  }, [])

  const t = useCallback<TFn>(
    (key, vars) => translate(locale, key, vars),
    [locale],
  )

  const formatNumber = useCallback(
    (value: number) => value.toLocaleString(numberLocale(locale)),
    [locale],
  )

  useEffect(() => {
    document.documentElement.lang = locale === 'zh' ? 'zh-CN' : 'en'
    void getCurrentWindow()
      .setTitle(translate(locale, 'appTitle'))
      .catch(() => {})
  }, [locale])

  const value = useMemo(
    () => ({ locale, setLocale, t, formatNumber }),
    [locale, setLocale, t, formatNumber],
  )

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

export function useI18n(): I18nValue {
  const ctx = useContext(I18nContext)
  if (!ctx) throw new Error('useI18n must be used within I18nProvider')
  return ctx
}
