"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

/**
 * Locale names match the codes the daemon ships in
 * `phonebridge-daemon/src/i18n.rs` (LOCALES). The set is
 * queried at runtime via `GET /api/v1/i18n` and the picker
 * populates from there; this client-side type is the
 * optimistic default before the API responds.
 */
export type Locale = "zh" | "en";
const STORAGE_KEY = "phonebridge.locale";

/** Response of `GET /api/v1/i18n`. */
interface I18nResponse {
  locale: string;
  strings: Record<string, string>;
  available: string[];
  default_locale: string;
}

// ---------------------------------------------------------------------------
// Module-level cache.
//
// We cache the dictionary keyed by the locale code so that
// (a) the next page navigation after a reload reuses the
// already-fetched strings (no second `fetch` round trip), and
// (b) React StrictMode's double-effect-invocation in dev does
// not produce two concurrent fetches for the same locale.
// The cache is read-only; a `setLocale` call writes to
// localStorage and triggers `window.location.reload()`, which
// re-mounts this module from scratch.
// ---------------------------------------------------------------------------
const dictionaryCache: Map<string, Record<string, string>> = new Map();
let availableLocales: string[] = ["zh", "en"];
let defaultSystemLocale: string = "en";
let cachePrimed = false;

/**
 * Fetch the dictionary for `locale` from the daemon. Returns
 * the cached entry if we have one — that is the common case
 * after the first load.
 */
async function fetchDictionary(locale: Locale): Promise<void> {
  // Always refresh the metadata (available, default_locale) on
  // a miss or before the first successful load; afterwards it
  // is also cached.
  const base =
    typeof window !== "undefined" ? window.location.origin : "";
  const res = await fetch(`${base}/api/v1/i18n?locale=${locale}`, {
    credentials: "omit",
  });
  if (!res.ok) throw new Error(`i18n fetch ${res.status}`);
  const json = (await res.json()) as I18nResponse;
  dictionaryCache.set(json.locale, json.strings);
  // Refresh the metadata fields. We deliberately do this
  // every time rather than memoising on locale, because
  // `available` and `default_locale` are daemon-wide and a
  // server restart could change them.
  availableLocales = json.available;
  defaultSystemLocale = json.default_locale;
  cachePrimed = true;
}

function readPersistedLocale(): Locale | null {
  if (typeof window === "undefined") return null;
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "zh" || saved === "en") return saved;
  } catch {
    // localStorage may throw under privacy mode / sandboxed
    // iframes — fall through.
  }
  return null;
}

function writePersistedLocale(l: Locale): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, l);
  } catch {
    // best-effort
  }
}

export type TranslationKey =
  | "nav.dashboard" | "nav.devices" | "nav.pairings" | "nav.notifications"
  | "nav.sms" | "nav.calls" | "nav.settings" | "nav.about"
  | "layout.console_subtitle" | "layout.online_count"
  | "dashboard.loading" | "dashboard.load_error"
  | "dashboard.title" | "dashboard.subtitle"
  | "dashboard.paired" | "dashboard.online" | "dashboard.unread"
  | "dashboard.unread_hint" | "dashboard.sms_conv" | "dashboard.sms_hint"
  | "dashboard.calls_24h" | "dashboard.calls_hint"
  | "dashboard.recent_notifs" | "dashboard.recent_sms" | "dashboard.recent_calls"
  | "dashboard.no_notifs" | "dashboard.no_sms" | "dashboard.no_calls"
  | "dashboard.view_all"
  | "live.title" | "live.live" | "live.disconnected"
  | "live.streamed_via" | "live.online_part" | "live.empty"
  | "live.ev_console_hello" | "live.ev_device_hello" | "live.ev_notif"
  | "live.ev_sms" | "live.ev_call_incoming" | "live.ev_call_state"
  | "live.ev_unpair"
  | "devices.title" | "devices.subtitle"
  | "devices.col.name" | "devices.col.paired" | "devices.col.last_seen"
  | "devices.loading" | "devices.empty"
  | "devices.badge_paired" | "devices.badge_discovered"
  | "devices.unpair_btn" | "devices.unpair_confirm"
  | "devices.unpair_btn_paired" | "devices.btn_pair" | "devices.btn_paired"
  | "pairings.title" | "pairings.subtitle" | "pairings.subtitle_hint"
  | "pairings.col.device" | "pairings.col.status" | "pairings.col.action"
  | "pairings.loading" | "pairings.empty"
  | "pairings.badge_paired" | "pairings.badge_unpaired"
  | "pairings.btn_pair" | "pairings.btn_paired"
  | "pairings.waiting_hint" | "pairings.waiting_cancel"
  | "pairings.start_info"
  | "pairings.incoming_title" | "pairings.incoming_hint"
  | "pairings.incoming_accept" | "pairings.incoming_reject"
  | "notif.title" | "notif.subtitle"
  | "notif.unread_only" | "notif.filter_placeholder"
  | "notif.loading" | "notif.empty"
  | "notif.sensitive" | "notif.hidden"
  | "notif.mark_read" | "notif.dismiss" | "notif.dismissing"
  | "notif.dismiss_title"
  | "sms.title" | "sms.subtitle" | "sms.all_devices"
  | "sms.conversations" | "sms.no_conversations"
  | "sms.pick_conversation" | "sms.no_conversation_selected" | "sms.no_messages"
  | "sms.placeholder_to" | "sms.placeholder_body"
  | "sms.send_via" | "sms.send" | "sms.select_device_first"
  | "calls.title" | "calls.subtitle" | "calls.place_call"
  | "calls.device" | "calls.number" | "calls.dial"
  | "calls.col.time" | "calls.col.number" | "calls.col.direction" | "calls.col.state" | "calls.col.duration" | "calls.col.sim"
  | "calls.loading" | "calls.empty"
  | "calls.dir_in" | "calls.dir_out" | "calls.dir_missed"
  | "settings.title" | "settings.subtitle"
  | "settings.col.time" | "settings.col.event" | "settings.col.device" | "settings.col.detail"
  | "settings.loading" | "settings.audit_title" | "settings.audit_empty"
  | "settings.language" | "settings.language_hint"
  | "settings.copy" | "settings.copied"
  | "about.title" | "about.subtitle" | "about.identity"
  | "about.col.id" | "about.col.name" | "about.col.fingerprint" | "about.col.pubkey" | "about.col.version"
  | "about.api_docs" | "about.api_docs_hint" | "about.source" | "about.license"
  | "lang.zh" | "lang.en";

/** Replace {name} placeholders in a translation string. */
function fillTemplate(
  s: string,
  vars?: Record<string, string | number>,
): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (_m, name) => {
    const v = vars[name];
    return v == null ? `{${name}}` : String(v);
  });
}

interface LocaleContextValue {
  locale: Locale;
  available: string[];
  setLocale: (l: Locale) => void;
  t: (key: TranslationKey, vars?: Record<string, string | number>) => string;
}

const LocaleContext = createContext<LocaleContextValue | null>(null);

/**
 * Loading shell — what the user sees while the first i18n
 * fetch is in flight. We deliberately keep the page chrome
 * (sidebar rail + centred spinner) the SAME structure as the
 * real layout so there is no shift when the dictionary lands.
 * No "nav.dashboard" / "Web Console" raw-key flash: the
 * children are NOT rendered until the dictionary is ready.
 */
function LoadingShell() {
  return (
    <div
      className="flex min-h-screen"
      // Match the (console) layout's structure so the page
      // doesn't jump when hydration finishes.
      suppressHydrationWarning
    >
      <aside className="sticky top-0 h-screen w-56 shrink-0 border-r border-base-300 bg-base-200 p-4 overflow-y-auto">
        <div className="mb-6 px-2">
          <div className="text-lg font-semibold text-brand-500">PhoneBridge</div>
          <div className="text-xs text-base-content/60 min-h-[1.25rem]">
            &nbsp;
          </div>
        </div>
      </aside>
      <main className="flex-1 p-6 min-w-0 flex items-center justify-center">
        <span className="loading loading-spinner loading-md opacity-60" />
      </main>
    </div>
  );
}

/**
 * Provider — boots up in two phases:
 *
 * 1. **Resolve the desired locale synchronously**: localStorage
 *    if the user has picked one before, otherwise the
 *    daemon's `default_locale` (which is the system LANG).
 *    No `navigator.language` fallback — the daemon's locale is
 *    the single source of truth.
 * 2. **Fetch the dictionary** (cache-hits are instant) and
 *    swap the loading shell out for the real UI in a single
 *    state transition. No raw-key flash.
 */
export function LocaleProvider({ children }: { children: ReactNode }) {
  const persisted = readPersistedLocale();

  // Synchronously read the cached dictionary for the desired
  // locale if we have one. This handles the "user came back
  // and hit reload" case: the module-level cache survives
  // across remounts within the same page-load, so a reload
  // shows zero flash.
  const initialCacheKey = persisted ?? "default";
  const [locale, setLocaleState] = useState<Locale>(
    persisted ?? "en", // optimistic; updated below if the daemon's default differs
  );
  const [strings, setStrings] = useState<Record<string, string> | null>(
    () => dictionaryCache.get(initialCacheKey) ?? null,
  );
  const [available, setAvailable] = useState<string[]>(availableLocales);
  const [hydrated, setHydrated] = useState<boolean>(strings !== null);

  useEffect(() => {
    let cancelled = false;
    // Decide the locale to fetch. Priority: persisted
    // localStorage → daemon's `default_locale` (from LANG).
    // Note: we do NOT use `navigator.language` here because
    // the console is opened from arbitrary browsers, and the
    // user's expectation is "match the daemon host's locale",
    // not "match the browser's locale".
    const target =
      persisted ??
      ((defaultSystemLocale as Locale) || "en");
    setLocaleState(target);
    // Cache hit: skip the network round-trip.
    if (dictionaryCache.has(target)) {
      if (cancelled) return;
      setStrings(dictionaryCache.get(target)!);
      setAvailable(availableLocales);
      setHydrated(true);
      return;
    }
    // Cache miss: fetch the dictionary.
    fetchDictionary(target)
      .then(() => {
        if (cancelled) return;
        setStrings(dictionaryCache.get(target) ?? {});
        setAvailable(availableLocales);
        setHydrated(true);
      })
      .catch(() => {
        // On a hard failure (daemon unreachable, CORS, etc.)
        // fall back to an empty dictionary and let `t()` show
        // raw keys. We do NOT block the UI indefinitely —
        // broken translations beat a blank screen.
        if (cancelled) return;
        setStrings({});
        setHydrated(true);
      });
    return () => {
      cancelled = true;
    };
  }, []); // intentionally empty: this is a one-shot bootstrap.

  // Keep <html lang> in sync so screen readers / browser
  // translation prompts see the active locale.
  useEffect(() => {
    if (typeof document === "undefined") return;
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }, [locale]);

  const setLocale = useCallback((l: Locale) => {
    writePersistedLocale(l);
    // Reload so every component re-renders against the
    // freshly-fetched dictionary; the module-level cache
    // guarantees the second page-load is instant for the
    // user.
    if (typeof window !== "undefined") {
      window.location.reload();
    }
  }, []);

  const t = useCallback(
    (key: TranslationKey, vars?: Record<string, string | number>) => {
      const raw = strings?.[key] ?? key;
      return fillTemplate(raw, vars);
    },
    [strings],
  );

  if (!hydrated) {
    return <LoadingShell />;
  }

  return (
    <LocaleContext.Provider value={{ locale, available, setLocale, t }}>
      {children}
    </LocaleContext.Provider>
  );
}

export function useLocale(): LocaleContextValue {
  const ctx = useContext(LocaleContext);
  if (ctx) return ctx;
  return {
    locale: "en",
    available: ["zh", "en"],
    setLocale: () => {},
    t: (key, vars) => fillTemplate(key, vars),
  };
}

/** Shorthand hook: just the `t` function. */
export function useT() {
  return useLocale().t;
}
