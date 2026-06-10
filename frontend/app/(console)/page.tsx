"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { api } from "@/lib/api";
import { LiveActivity } from "@/components/LiveActivity";
import { useT } from "@/lib/i18n";

function StatCard({ label, value, hint }: { label: string; value: string | number; hint?: string }) {
  return (
    <div className="card bg-base-200">
      <div className="card-body p-4">
        <div className="text-sm text-base-content/60">{label}</div>
        <div className="text-3xl font-semibold">{value}</div>
        {hint && <div className="text-xs text-base-content/50">{hint}</div>}
      </div>
    </div>
  );
}

function Spinner() {
  return <span className="loading loading-spinner loading-sm" />;
}

function ErrorBox({ message }: { message: string }) {
  return (
    <div className="alert alert-error">
      <span>{message}</span>
    </div>
  );
}

export default function DashboardPage() {
  const t = useT();
  const dashboard = useQuery({ queryKey: ["dashboard", "page"], queryFn: () => api.dashboard(), refetchInterval: 5_000 });
  const recentCalls = useQuery({
    queryKey: ["calls", "recent"],
    queryFn: () => api.calls({ limit: 5 }),
    refetchInterval: 10_000,
  });
  const recentNotifs = useQuery({
    queryKey: ["notifications", "recent"],
    queryFn: () => api.notifications({ limit: 5 }),
    refetchInterval: 5_000,
  });
  const recentSms = useQuery({
    queryKey: ["sms", "recent"],
    queryFn: () => api.sms({ limit: 5 }),
    refetchInterval: 5_000,
  });

  if (dashboard.isLoading) {
    return (
      <div className="flex items-center gap-2">
        <Spinner /> {t("dashboard.loading")}
      </div>
    );
  }
  if (dashboard.isError || !dashboard.data) {
    return <ErrorBox message={(dashboard.error as Error)?.message ?? t("dashboard.load_error")} />;
  }
  const d = dashboard.data;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("dashboard.title")}</h1>
        <p className="text-sm text-base-content/60">{t("dashboard.subtitle")}</p>
      </header>

      <section className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-5">
        <StatCard label={t("dashboard.paired")} value={d.paired_devices} />
        <StatCard label={t("dashboard.online")} value={d.online_devices} />
        <StatCard
          label={t("dashboard.unread")}
          value={d.notifications.unread}
          hint={t("dashboard.unread_hint", { n: d.notifications.total })}
        />
        <StatCard
          label={t("dashboard.sms_conv")}
          value={d.sms.conversations}
          hint={t("dashboard.sms_hint", { n: d.sms.total })}
        />
        <StatCard
          label={t("dashboard.calls_24h")}
          value={d.calls.total}
          hint={t("dashboard.calls_hint", { ringing: d.calls.ringing, missed: d.calls.missed })}
        />
      </section>

      <section className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="card bg-base-200">
          <div className="card-body p-4">
            <h2 className="card-title text-base">{t("dashboard.recent_notifs")}</h2>
            {recentNotifs.data?.notifications.length === 0 ? (
              <p className="text-sm text-base-content/50">{t("dashboard.no_notifs")}</p>
            ) : (
              <ul className="space-y-1 text-sm">
                {recentNotifs.data?.notifications.map((n) => (
                  <li key={`${n.device_id}-${n.id}`} className="truncate">
                    <span className="opacity-50">[{n.app_name ?? n.package_name}]</span>{" "}
                    {n.is_sensitive ? <em>{t("notif.hidden")}</em> : n.title}
                  </li>
                ))}
              </ul>
            )}
            <Link href="/notifications/" className="link link-primary mt-2 text-sm">
              {t("dashboard.view_all")}
            </Link>
          </div>
        </div>

        <div className="card bg-base-200">
          <div className="card-body p-4">
            <h2 className="card-title text-base">{t("dashboard.recent_sms")}</h2>
            {recentSms.data?.messages.length === 0 ? (
              <p className="text-sm text-base-content/50">{t("dashboard.no_sms")}</p>
            ) : (
              <ul className="space-y-1 text-sm">
                {recentSms.data?.messages.map((s) => (
                  <li key={s.id} className="truncate">
                    <span className="opacity-50">[{s.direction === "in" ? "←" : "→"}]</span> {s.phone_number}: {s.body}
                  </li>
                ))}
              </ul>
            )}
            <Link href="/sms/" className="link link-primary mt-2 text-sm">
              {t("dashboard.view_all")}
            </Link>
          </div>
        </div>

        <div className="card bg-base-200">
          <div className="card-body p-4">
            <h2 className="card-title text-base">{t("dashboard.recent_calls")}</h2>
            {recentCalls.data?.calls.length === 0 ? (
              <p className="text-sm text-base-content/50">{t("dashboard.no_calls")}</p>
            ) : (
              <ul className="space-y-1 text-sm">
                {recentCalls.data?.calls.map((c) => (
                  <li key={c.id} className="truncate">
                    <span className="opacity-50">[{c.direction}]</span> {c.phone_number} ·{" "}
                    {c.state}
                  </li>
                ))}
              </ul>
            )}
            <Link href="/calls/" className="link link-primary mt-2 text-sm">
              {t("dashboard.view_all")}
            </Link>
          </div>
        </div>

        <LiveActivity />
      </section>
    </div>
  );
}
