"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, type Device } from "@/lib/api";

export default function PairingsPage() {
  const qc = useQueryClient();
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: () => api.devices(),
    refetchInterval: 5_000,
  });
  const startPair = useMutation({
    mutationFn: (device_id: string) => api.startPair(device_id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pairings"] }),
  });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Pairings</h1>
        <p className="text-sm text-base-content/60">
          Click <b>Pair</b> on a device below to send a <code>device.pair.request</code> over the
          open WebSocket. The Android client will display a 6-digit code on its screen; once the user
          confirms, the pairing completes automatically.
        </p>
      </header>

      <div className="card bg-base-200">
        <div className="card-body p-0">
          <table className="table">
            <thead>
              <tr>
                <th>Device</th>
                <th>Paired</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {devices.isLoading && (
                <tr>
                  <td colSpan={3} className="text-center text-sm opacity-60">
                    Loading…
                  </td>
                </tr>
              )}
              {devices.data?.devices.length === 0 && (
                <tr>
                  <td colSpan={3} className="text-center text-sm opacity-60">
                    No devices yet. Open the PhoneBridge Android app to make it appear here.
                  </td>
                </tr>
              )}
              {devices.data?.devices.map((d: Device) => (
                <tr key={d.id}>
                  <td>
                    <div className="font-medium">{d.name}</div>
                    <div className="font-mono text-[10px] opacity-50">{d.device_id}</div>
                  </td>
                  <td>
                    {d.paired ? (
                      <span className="badge badge-success badge-sm">paired</span>
                    ) : (
                      <span className="badge badge-ghost badge-sm">unpaired</span>
                    )}
                  </td>
                  <td>
                    <button
                      className="btn btn-primary btn-sm"
                      disabled={d.paired || startPair.isPending}
                      onClick={() => startPair.mutate(d.device_id)}
                    >
                      {d.paired ? "Already paired" : "Pair"}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {startPair.data && (
        <div className="alert alert-info">
          <span>
            Pair request sent to {startPair.data.device_id}. Check the Android screen for the
            6-digit code.
          </span>
        </div>
      )}
      {startPair.error && (
        <div className="alert alert-error">
          <span>{(startPair.error as Error).message}</span>
        </div>
      )}
    </div>
  );
}
