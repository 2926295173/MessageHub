"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, type Call, type Device } from "@/lib/api";
import { useT } from "@/lib/i18n";

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

function fmtDuration(secs: number | null): string {
  if (secs == null) return "—";
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}m ${s}s`;
}

export default function CallsPage() {
  const t = useT();
  const qc = useQueryClient();
  const [deviceId, setDeviceId] = useState<string | "">("");
  const [dialNumber, setDialNumber] = useState("");
  const [dialError, setDialError] = useState<string | null>(null);

  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: () => api.devices(),
    refetchInterval: 30_000,
  });
  const calls = useQuery({
    queryKey: ["calls", "page", deviceId],
    queryFn: () => api.calls({ device_id: deviceId || undefined, limit: 200 }),
    refetchInterval: 5_000,
  });

  const dial = useMutation({
    mutationFn: ({ device_id, number }: { device_id: string; number: string }) =>
      api.dial(device_id, number),
    onSuccess: () => {
      setDialNumber("");
      setDialError(null);
      qc.invalidateQueries({ queryKey: ["calls"] });
    },
    onError: (e) => setDialError((e as Error).message),
  });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("calls.title")}</h1>
        <p className="text-sm text-base-content/60">{t("calls.subtitle")}</p>
      </header>

      <div className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">{t("calls.place_call")}</h2>
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (!deviceId || !dialNumber) return;
              dial.mutate({ device_id: deviceId, number: dialNumber });
            }}
          >
            <label className="form-control">
              <span className="label-text text-xs">{t("calls.device")}</span>
              <select
                className="select select-bordered select-sm"
                value={deviceId}
                onChange={(e) => setDeviceId(e.target.value)}
              >
                <option value="">{t("calls.device")}…</option>
                {devices.data?.devices.map((d: Device) => (
                  <option key={d.device_id} value={d.device_id}>
                    {d.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="form-control flex-1">
              <span className="label-text text-xs">{t("calls.number")}</span>
              <input
                type="tel"
                className="input input-bordered input-sm w-full"
                placeholder="+8613800000000"
                value={dialNumber}
                onChange={(e) => setDialNumber(e.target.value)}
              />
            </label>
            <button
              type="submit"
              className="btn btn-primary btn-sm"
              disabled={!deviceId || !dialNumber || dial.isPending}
            >
              {t("calls.dial")}
            </button>
          </form>
          {dialError && <div className="alert alert-error mt-2 text-xs">{dialError}</div>}
        </div>
      </div>

      <div className="card bg-base-200">
        <div className="card-body p-0">
          <table className="table">
            <thead>
              <tr>
                <th>{t("calls.col.time")}</th>
                <th>{t("calls.col.number")}</th>
                <th>{t("calls.col.direction")}</th>
                <th>{t("calls.col.state")}</th>
                <th>{t("calls.col.duration")}</th>
                <th>{t("calls.col.sim")}</th>
              </tr>
            </thead>
            <tbody>
              {calls.isLoading && (
                <tr>
                  <td colSpan={6} className="text-center text-sm opacity-60">
                    {t("calls.loading")}
                  </td>
                </tr>
              )}
              {calls.data?.calls.length === 0 && (
                <tr>
                  <td colSpan={6} className="text-center text-sm opacity-60">
                    {t("calls.empty")}
                  </td>
                </tr>
              )}
              {calls.data?.calls.map((c: Call) => (
                <tr key={c.id}>
                  <td className="text-xs opacity-70">{fmtTime(c.started_at)}</td>
                  <td>{c.phone_number}</td>
                  <td>
                    <span
                      className={`badge badge-sm ${
                        c.direction === "incoming"
                          ? "badge-info"
                          : c.direction === "outgoing"
                            ? "badge-success"
                            : "badge-warning"
                      }`}
                    >
                      {c.direction === "incoming" ? t("calls.dir_in") : c.direction === "outgoing" ? t("calls.dir_out") : c.direction}
                    </span>
                  </td>
                  <td>{c.state}</td>
                  <td>{fmtDuration(c.duration_secs)}</td>
                  <td className="font-mono text-xs opacity-60">{c.sim_slot ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
