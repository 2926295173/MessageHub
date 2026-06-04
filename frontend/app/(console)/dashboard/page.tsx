"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { api } from "@/lib/api";

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
        <Spinner /> Loading dashboard…
      </div>
    );
  }
  if (dashboard.isError || !dashboard.data) {
    return <ErrorBox message={(dashboard.error as Error)?.message ?? "failed to load"} />;
  }
  const d = dashboard.data;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Dashboard</h1>
        <p className="text-sm text-base-content/60">Live snapshot of all paired Android devices.</p>
      </header>

      <section className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-5">
        <StatCard label="Paired devices" value={d.paired_devices} />
        <StatCard label="Online now" value={d.online_devices} />
        <StatCard
          label="Unread notifications"
          value={d.notifications.unread}
          hint={`${d.notifications.total} total`}
        />
        <StatCard
          label="SMS conversations"
          value={d.sms.conversations}
          hint={`${d.sms.total} messages`}
        />
        <StatCard
          label="Calls (24h-style)"
          value={d.calls.total}
          hint={`${d.calls.ringing} ringing · ${d.calls.missed} missed`}
        />
      </section>

      <section className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="card bg-base-200">
          <div className="card-body p-4">
            <h2 className="card-title text-base">Recent notifications</h2>
            {recentNotifs.data?.notifications.length === 0 ? (
              <p className="text-sm text-base-content/50">No notifications yet.</p>
            ) : (
              <ul className="space-y-1 text-sm">
                {recentNotifs.data?.notifications.map((n) => (
                  <li key={`${n.device_id}-${n.id}`} className="truncate">
                    <span className="opacity-50">[{n.app_name ?? n.package_name}]</span>{" "}
                    {n.is_sensitive ? <em>(hidden)</em> : n.title}
                  </li>
                ))}
              </ul>
            )}
            <Link href="/notifications/" className="link link-primary mt-2 text-sm">
              View all →
            </Link>
          </div>
        </div>

        <div className="card bg-base-200">
          <div className="card-body p-4">
            <h2 className="card-title text-base">Recent SMS</h2>
            {recentSms.data?.messages.length === 0 ? (
              <p className="text-sm text-base-content/50">No SMS yet.</p>
            ) : (
              <ul className="space-y-1 text-sm">
                {recentSms.data?.messages.map((s) => (
                  <li key={s.id} className="truncate">
                    <span className="opacity-50">[{s.direction}]</span> {s.phone_number}: {s.body}
                  </li>
                ))}
              </ul>
            )}
            <Link href="/sms/" className="link link-primary mt-2 text-sm">
              View all →
            </Link>
          </div>
        </div>

        <div className="card bg-base-200">
          <div className="card-body p-4">
            <h2 className="card-title text-base">Recent calls</h2>
            {recentCalls.data?.calls.length === 0 ? (
              <p className="text-sm text-base-content/50">No calls yet.</p>
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
              View all →
            </Link>
          </div>
        </div>
      </section>
    </div>
  );
}
