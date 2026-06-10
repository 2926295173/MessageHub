"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useLocale, useT, type Locale } from "@/lib/i18n";

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

/**
 * Settings — language picker + audit log. The daemon's identity
 * (device id, name, fingerprint, public key) intentionally lives
 * on the About page instead of here, so a user who shares the
 * Settings screen for support purposes doesn't accidentally show
 * the long-term TLS fingerprint to whoever they're screensharing
 * with.
 */
export default function SettingsPage() {
  const t = useT();
  const { locale, setLocale, available } = useLocale();
  const audit = useQuery({
    queryKey: ["audit"],
    queryFn: () => api.audit({ limit: 50 }),
  });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("settings.title")}</h1>
        <p className="text-sm text-base-content/60">{t("settings.subtitle")}</p>
      </header>

      {/* Language picker — the only setting on this page. */}
      <section className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">{t("settings.language")}</h2>
          <p className="text-sm text-base-content/60">{t("settings.language_hint")}</p>
          <div className="mt-2 join">
            {available.map((l) => {
              const code = l as Locale;
              const labelKey = (code === "zh" ? "lang.zh" : "lang.en") as
                | "lang.zh"
                | "lang.en";
              return (
                <button
                  key={l}
                  type="button"
                  onClick={() => setLocale(code)}
                  className={`btn join-item btn-sm ${
                    locale === code ? "btn-primary" : "btn-ghost"
                  }`}
                  aria-pressed={locale === code}
                >
                  {t(labelKey)}
                </button>
              );
            })}
          </div>
        </div>
      </section>

      {/* Audit log — read-only, useful when triaging "why did X happen". */}
      <section className="card bg-base-200">
        <div className="card-body p-0">
          <div className="border-b border-base-300 p-3 text-sm font-semibold">
            {t("settings.audit_title")}
          </div>
          <table className="table table-zebra">
            <thead>
              <tr>
                <th>{t("settings.col.time")}</th>
                <th>{t("settings.col.event")}</th>
                <th>{t("settings.col.device")}</th>
                <th>{t("settings.col.detail")}</th>
              </tr>
            </thead>
            <tbody>
              {audit.isLoading && (
                <tr>
                  <td colSpan={4} className="text-center text-sm opacity-60">
                    {t("settings.loading")}
                  </td>
                </tr>
              )}
              {audit.data?.entries.length === 0 && (
                <tr>
                  <td colSpan={4} className="text-center text-sm opacity-60">
                    {t("settings.audit_empty")}
                  </td>
                </tr>
              )}
              {audit.data?.entries.map((e) => (
                <tr key={e.id}>
                  <td className="text-xs opacity-70">{fmtTime(e.timestamp)}</td>
                  <td>
                    <span className="badge badge-ghost badge-sm">{e.event}</span>
                  </td>
                  <td className="font-mono text-[10px] opacity-60">
                    {e.device_id?.slice(0, 8) ?? "—"}…
                  </td>
                  <td className="text-xs opacity-70">{e.detail ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
