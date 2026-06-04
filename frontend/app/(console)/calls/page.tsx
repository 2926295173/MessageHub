"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, type Call, type Device } from "@/lib/api";

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
        <h1 className="text-2xl font-semibold">Calls</h1>
        <p className="text-sm text-base-content/60">
          Recent call log from your Android devices. You can also place outgoing calls.
        </p>
      </header>

      <div className="card bg-base-200">
        <div className="card-body p-4">
          <h2 className="card-title text-base">Place a call</h2>
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (!deviceId || !dialNumber) return;
              dial.mutate({ device_id: deviceId, number: dialNumber });
            }}
          >
            <label className="form-control">
              <span className="label-text text-xs">Device</span>
              <select
                className="select select-bordered select-sm"
                value={deviceId}
                onChange={(e) => setDeviceId(e.target.value)}
              >
                <option value="">Pick a device…</option>
                {devices.data?.devices.map((d: Device) => (
                  <option key={d.device_id} value={d.device_id}>
                    {d.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="form-control flex-1">
              <span className="label-text text-xs">Number</span>
              <input
                type="tel"
                className="input input-bordered input-sm w-full"
                placeholder="+1234567890"
                value={dialNumber}
                onChange={(e) => setDialNumber(e.target.value)}
              />
            </label>
            <button
              type="submit"
              className="btn btn-primary btn-sm"
              disabled={!deviceId || !dialNumber || dial.isPending}
            >
              Dial
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
                <th>Time</th>
                <th>Number</th>
                <th>Direction</th>
                <th>State</th>
                <th>Duration</th>
                <th>SIM</th>
              </tr>
            </thead>
            <tbody>
              {calls.isLoading && (
                <tr>
                  <td colSpan={6} className="text-center text-sm opacity-60">
                    Loading…
                  </td>
                </tr>
              )}
              {calls.data?.calls.length === 0 && (
                <tr>
                  <td colSpan={6} className="text-center text-sm opacity-60">
                    No calls yet.
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
                      {c.direction}
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
