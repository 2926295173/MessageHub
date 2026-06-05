"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useRef } from "react";
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
  const acceptPair = useMutation({
    mutationFn: ({ device_id, code }: { device_id: string; code: string }) =>
      api.acceptPair(device_id, code),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pairings"] }),
  });
  const rejectPair = useMutation({
    mutationFn: ({ device_id, reason }: { device_id: string; reason: string }) =>
      api.rejectPair(device_id, reason),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pairings"] }),
  });

  // Track which device is currently being paired, plus the typed code
  // and the last error.
  const [pendingDevice, setPendingDevice] = useState<Device | null>(null);
  const [code, setCode] = useState("");
  const codeInputRef = useRef<HTMLInputElement | null>(null);

  const beginPair = (d: Device) => {
    setPendingDevice(d);
    setCode("");
    startPair.mutate(d.device_id, {
      onSuccess: () => {
        // Focus the code input after a tick so DaisyUI can mount it.
        setTimeout(() => codeInputRef.current?.focus(), 50);
      },
    });
  };

  const onAccept = () => {
    if (!pendingDevice) return;
    if (code.length !== 6 || !/^\d{6}$/.test(code)) return;
    acceptPair.mutate(
      { device_id: pendingDevice.device_id, code },
      {
        onSuccess: () => {
          setPendingDevice(null);
          setCode("");
        },
      }
    );
  };

  const onReject = () => {
    if (!pendingDevice) return;
    rejectPair.mutate(
      { device_id: pendingDevice.device_id, reason: "rejected from console" },
      {
        onSuccess: () => {
          setPendingDevice(null);
          setCode("");
        },
      }
    );
  };

  const cancelPending = () => {
    setPendingDevice(null);
    setCode("");
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Pairings</h1>
        <p className="text-sm text-base-content/60">
          Click <b>Pair</b> on a device below. A 6-digit code will appear on the Android
          device; type it here and click <b>Accept</b> to complete pairing. The Android
          client uses ECDH P-256 + HKDF-SHA256 to derive the code from the shared secret
          (matches <code>docs/protocol-v1.md</code> §4.2).
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
                    {pendingDevice?.device_id === d.device_id ? (
                      <span className="text-xs opacity-60">code-entry open ↓</span>
                    ) : (
                      <button
                        className="btn btn-primary btn-sm"
                        disabled={d.paired || startPair.isPending}
                        onClick={() => beginPair(d)}
                      >
                        {d.paired ? "Already paired" : "Pair"}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {pendingDevice && (
        <div className="card bg-base-200">
          <div className="card-body">
            <h2 className="card-title">Type the 6-digit code</h2>
            <p className="text-sm text-base-content/60">
              The Android device <span className="font-mono">{pendingDevice.name}</span>{" "}
              is showing a 6-digit code. Read it from the device screen and type it here.
              The desktop will then send <code>device.pair.accept</code>.
            </p>
            <div className="form-control w-full max-w-sm">
              <label className="label">
                <span className="label-text">6-digit code (digits only)</span>
              </label>
              <input
                ref={codeInputRef}
                type="text"
                inputMode="numeric"
                pattern="\d{6}"
                maxLength={6}
                className="input input-bordered text-2xl tracking-[0.5em] font-mono text-center"
                value={code}
                onChange={(e) => {
                  const v = e.target.value.replace(/\D/g, "").slice(0, 6);
                  setCode(v);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && code.length === 6) onAccept();
                }}
                autoComplete="off"
                placeholder="000000"
              />
            </div>
            <div className="card-actions mt-2">
              <button
                className="btn btn-success"
                onClick={onAccept}
                disabled={code.length !== 6 || acceptPair.isPending}
              >
                {acceptPair.isPending ? "Accepting…" : "Accept"}
              </button>
              <button
                className="btn btn-ghost"
                onClick={onReject}
                disabled={rejectPair.isPending}
              >
                {rejectPair.isPending ? "Rejecting…" : "Reject"}
              </button>
              <button className="btn btn-ghost" onClick={cancelPending}>
                Cancel
              </button>
            </div>
            {acceptPair.error && (
              <div className="alert alert-error mt-2 text-sm">
                {(acceptPair.error as Error).message}
              </div>
            )}
          </div>
        </div>
      )}

      {startPair.data && !pendingDevice && (
        <div className="alert alert-info">
          <span>
            Pair request sent to {startPair.data.device_id}. Check the Android screen for
            the 6-digit code.
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
