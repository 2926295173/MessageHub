"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

export default function SettingsPage() {
  const cert = useQuery({ queryKey: ["cert"], queryFn: () => api.cert() });
  const audit = useQuery({ queryKey: ["audit"], queryFn: () => api.audit({ limit: 50 }) });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Settings</h1>
        <p className="text-sm text-base-content/60">
          This daemon's identity and recent activity.
        </p>
      </header>

      <section className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">This daemon</h2>
          {cert.data ? (
            <dl className="grid grid-cols-1 gap-2 text-sm md:grid-cols-2">
              <Field label="Device id" value={cert.data.device_id} mono />
              <Field label="Name" value={cert.data.name} />
              <Field label="Fingerprint" value={cert.data.fingerprint} mono />
              <Field label="Public key" value={cert.data.public_key} mono />
            </dl>
          ) : (
            <div className="text-sm opacity-60">Loading…</div>
          )}
        </div>
      </section>

      <section className="card bg-base-200">
        <div className="card-body p-0">
          <div className="border-b border-base-300 p-3 text-sm font-semibold">
            Recent audit log
          </div>
          <table className="table table-zebra">
            <thead>
              <tr>
                <th>Time</th>
                <th>Event</th>
                <th>Device</th>
                <th>Detail</th>
              </tr>
            </thead>
            <tbody>
              {audit.isLoading && (
                <tr>
                  <td colSpan={4} className="text-center text-sm opacity-60">
                    Loading…
                  </td>
                </tr>
              )}
              {audit.data?.entries.length === 0 && (
                <tr>
                  <td colSpan={4} className="text-center text-sm opacity-60">
                    No entries yet.
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

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <dt className="text-xs uppercase opacity-50">{label}</dt>
      <dd className={mono ? "break-all font-mono text-xs" : "text-sm"}>{value}</dd>
    </div>
  );
}
