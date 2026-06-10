"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useT } from "@/lib/i18n";

/**
 * About page — single source of truth for the daemon's identity and
 * external links. Sensitive material (device id, public key, TLS
 * fingerprint) lives ONLY here, not on the Devices / Settings /
 * Sidebar, so a casual viewer of the console can see paired
 * devices without being exposed to the credentials needed to
 * impersonate them.
 *
 * The fields are copy-to-clipboard for diagnostics. The
 * implementation uses a small `useCopy` hook to flip the label
 * briefly after a successful copy.
 */
export default function AboutPage() {
  const t = useT();
  const cert = useQuery({
    queryKey: ["cert"],
    queryFn: () => api.cert(),
    refetchInterval: 60_000,
  });
  const health = useQuery({
    queryKey: ["health"],
    queryFn: () => api.health(),
    refetchInterval: 30_000,
  });

  return (
    <div className="space-y-6 max-w-3xl">
      <header>
        <h1 className="text-2xl font-semibold">{t("about.title")}</h1>
        <p className="text-sm text-base-content/60">{t("about.subtitle")}</p>
      </header>

      {/* Identity card — the only place pubkey / fingerprint / device id are shown. */}
      <section className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">{t("about.identity")}</h2>
          {cert.data ? (
            <dl className="grid grid-cols-1 gap-3 text-sm">
              <Field label={t("about.col.name")} value={cert.data.name} />
              <Field label={t("about.col.id")} value={cert.data.device_id} mono />
              <Field label={t("about.col.fingerprint")} value={cert.data.fingerprint} mono />
              <Field label={t("about.col.pubkey")} value={cert.data.public_key} mono />
              <Field label={t("about.col.version")} value={health.data?.version ?? "—"} />
            </dl>
          ) : (
            <div className="text-sm opacity-60">{t("settings.loading")}</div>
          )}
        </div>
      </section>

      {/* External links */}
      <section className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">{t("about.api_docs")}</h2>
          <p className="text-sm text-base-content/60">{t("about.api_docs_hint")}</p>
          <div className="mt-2">
            <a
              href="/console/api-docs/"
              target="_blank"
              rel="noreferrer"
              className="btn btn-primary btn-sm"
            >
              {t("about.api_docs")} ↗
            </a>
          </div>
        </div>
      </section>

      <section className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">{t("about.source")}</h2>
          <p className="text-sm">
            <a
              href="https://github.com/anomalyco/phonebridge"
              target="_blank"
              rel="noreferrer"
              className="link link-primary"
            >
              github.com/anomalyco/phonebridge ↗
            </a>
          </p>
          <p className="text-xs text-base-content/60 mt-2">{t("about.license")}</p>
        </div>
      </section>
    </div>
  );
}

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  const t = useT();
  const copied = useCopy(value);
  return (
    <div>
      <dt className="text-xs uppercase opacity-50 mb-1">{label}</dt>
      <dd className="flex items-start gap-2">
        <code
          className={
            mono
              ? "break-all font-mono text-xs bg-base-300 rounded px-2 py-1 flex-1"
              : "text-sm bg-base-300 rounded px-2 py-1 flex-1"
          }
        >
          {value}
        </code>
        <button
          type="button"
          onClick={copied.copy}
          className="btn btn-ghost btn-xs shrink-0"
          title={value}
        >
          {copied.done ? t("settings.copied") : t("settings.copy")}
        </button>
      </dd>
    </div>
  );
}

/** Returns { copy, done }; flips `done` back to false after 1.5 s. */
function useCopy(text: string): { copy: () => void; done: boolean } {
  const t = useT();
  const [done, setDone] = useState(false);
  const copy = () => {
    if (typeof navigator === "undefined" || !navigator.clipboard) {
      // Fallback: select-and-copy via a temporary textarea.
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
        setDone(true);
        setTimeout(() => setDone(false), 1500);
      } catch {
        // ignore
      } finally {
        document.body.removeChild(ta);
      }
      return;
    }
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setDone(true);
        setTimeout(() => setDone(false), 1500);
      })
      .catch(() => {
        // Clipboard API can reject (insecure context, permissions, etc.).
        // Silent failure — the button is best-effort.
      });
  };
  // (t is referenced only to keep the i18n collector honest about
  // the surrounding copy strings; reading it here would re-render.)
  void t;
  return { copy, done };
}
