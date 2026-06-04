"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, type Device } from "@/lib/api";

function fmtTime(t: number): string {
  if (!t) return "—";
  return new Date(t * 1000).toLocaleString();
}

export default function DevicesPage() {
  const qc = useQueryClient();
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: () => api.devices(),
    refetchInterval: 5_000,
  });
  const dashboard = useQuery({
    queryKey: ["dashboard", "sidebar"],
    queryFn: () => api.dashboard(),
    refetchInterval: 5_000,
  });
  const online = new Set(
    // (we approximate by reading the dashboard's online count; a future
    // improvement is to expose the actual connected ids via /devices)
    [],
  );
  const remove = useMutation({
    mutationFn: (id: string) => api.removeDevice(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
  });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Devices</h1>
        <p className="text-sm text-base-content/60">
          Android clients paired with this daemon. {dashboard.data?.online_devices ?? 0} currently
          online.
        </p>
      </header>

      <div className="card bg-base-200">
        <div className="card-body p-0">
          <table className="table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Device id</th>
                <th>Paired</th>
                <th>Last seen</th>
                <th>Public key (truncated)</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {devices.isLoading && (
                <tr>
                  <td colSpan={6} className="text-center text-sm opacity-60">
                    Loading…
                  </td>
                </tr>
              )}
              {devices.data?.devices.length === 0 && (
                <tr>
                  <td colSpan={6} className="text-center text-sm opacity-60">
                    No devices yet. Open the PhoneBridge Android app to pair.
                  </td>
                </tr>
              )}
              {devices.data?.devices.map((d: Device) => (
                <tr key={d.id}>
                  <td>{d.name}</td>
                  <td className="font-mono text-xs opacity-60">
                    {d.device_id.slice(0, 8)}…
                  </td>
                  <td>
                    {d.paired ? (
                      <span className="badge badge-success badge-sm">paired</span>
                    ) : (
                      <span className="badge badge-ghost badge-sm">discovered</span>
                    )}
                  </td>
                  <td className="text-xs opacity-60">{fmtTime(d.last_seen)}</td>
                  <td className="font-mono text-[10px] opacity-50">
                    {d.public_key.slice(0, 20)}…
                  </td>
                  <td>
                    <button
                      className="btn btn-ghost btn-xs text-error"
                      disabled={remove.isPending}
                      onClick={() => {
                        if (confirm(`Unpair ${d.name}?`)) remove.mutate(d.device_id);
                      }}
                    >
                      Unpair
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
