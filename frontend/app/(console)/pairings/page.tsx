"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, type Device } from "@/lib/api";
import { useT } from "@/lib/i18n";

const PAIRING_TIMEOUT_SECS = 30;

export default function PairingsPage() {
  const t = useT();
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
  const rejectPair = useMutation({
    mutationFn: ({ device_id, reason }: { device_id: string; reason: string }) =>
      api.rejectPair(device_id, reason),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pairings"] }),
  });
  const incoming = useQuery({
    queryKey: ["pairings", "incoming"],
    queryFn: () => api.incomingPairings(),
    refetchInterval: 3_000,
  });
  const incomingAccept = useMutation({
    mutationFn: (device_id: string) => api.incomingAccept(device_id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pairings", "incoming"] }),
  });
  const incomingReject = useMutation({
    mutationFn: (device_id: string) => api.incomingReject(device_id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pairings", "incoming"] }),
  });

  // Track the device currently being paired, plus a countdown. The
  // desktop no longer inputs a verification code — the code lives on
  // the phone (the trusted UI surface) and the user clicks Accept
  // there. The desktop's job is just to wait for the phone's
  // `device.pair.confirm(true)` and then exchange certs.
  const [pendingDevice, setPendingDevice] = useState<Device | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [remaining, setRemaining] = useState<number>(PAIRING_TIMEOUT_SECS);

  useEffect(() => {
    if (!pendingDevice || startedAt == null) return;
    const tick = () => {
      const left = Math.max(
        0,
        PAIRING_TIMEOUT_SECS - Math.floor((Date.now() - startedAt) / 1000),
      );
      setRemaining(left);
      if (left === 0) {
        setPendingDevice(null);
        setStartedAt(null);
      }
    };
    tick();
    const id = setInterval(tick, 500);
    return () => clearInterval(id);
  }, [pendingDevice, startedAt]);

  // Once the device row flips to "已配对" in the poll, clear the
  // pending state automatically.
  useEffect(() => {
    if (!pendingDevice) return;
    const matched = devices.data?.devices.find(
      (d) => d.device_id === pendingDevice.device_id,
    );
    if (matched?.paired) {
      setPendingDevice(null);
      setStartedAt(null);
    }
  }, [devices.data, pendingDevice]);

  const beginPair = (d: Device) => {
    setPendingDevice(d);
    setStartedAt(Date.now());
    setRemaining(PAIRING_TIMEOUT_SECS);
    startPair.mutate(d.device_id);
  };

  const onCancel = () => {
    if (!pendingDevice) return;
    rejectPair.mutate(
      { device_id: pendingDevice.device_id, reason: "cancelled from console" },
      {
        onSettled: () => {
          setPendingDevice(null);
          setStartedAt(null);
        },
      },
    );
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("pairings.title")}</h1>
        <p className="text-sm text-base-content/60">
          {t("pairings.subtitle")} {t("pairings.subtitle_hint")}
        </p>
      </header>

      {incoming.data?.pending && incoming.data.pending.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-lg font-semibold">{t("pairings.incoming_title")}</h2>
          {incoming.data.pending.map((p) => (
            <div key={p.device_id} className="card bg-warning/20 border border-warning">
              <div className="card-body p-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <div className="font-semibold">
                      {p.name}{" "}
                      <span className="opacity-50 text-sm">
                        {t("pairings.incoming_title").replace("入站", "").replace("Incoming", "")}
                      </span>
                    </div>
                    <div className="font-mono text-[10px] opacity-50">{p.device_id}</div>
                    <div className="text-xs opacity-60 mt-1">
                      {t("pairings.incoming_hint")}
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <button
                      className="btn btn-ghost btn-sm"
                      disabled={incomingReject.isPending}
                      onClick={() => incomingReject.mutate(p.device_id)}
                    >
                      {t("pairings.incoming_reject")}
                    </button>
                    <button
                      className="btn btn-success btn-sm"
                      disabled={incomingAccept.isPending}
                      onClick={() => incomingAccept.mutate(p.device_id)}
                    >
                      {t("pairings.incoming_accept")}
                    </button>
                  </div>
                </div>
                {(incomingAccept.error || incomingReject.error) && (
                  <div className="alert alert-error mt-2 text-sm">
                    {((incomingAccept.error || incomingReject.error) as Error).message}
                  </div>
                )}
              </div>
            </div>
          ))}
        </section>
      )}

      <div className="card bg-base-200">
        <div className="card-body p-0">
          <table className="table">
            <thead>
              <tr>
                <th>{t("pairings.col.device")}</th>
                <th>{t("pairings.col.status")}</th>
                <th>{t("pairings.col.action")}</th>
              </tr>
            </thead>
            <tbody>
              {devices.isLoading && (
                <tr>
                  <td colSpan={3} className="text-center text-sm opacity-60">
                    {t("pairings.loading")}
                  </td>
                </tr>
              )}
              {devices.data?.devices.length === 0 && (
                <tr>
                  <td colSpan={3} className="text-center text-sm opacity-60">
                    {t("pairings.empty")}
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
                      <span className="badge badge-success badge-sm">{t("pairings.badge_paired")}</span>
                    ) : (
                      <span className="badge badge-ghost badge-sm">{t("pairings.badge_unpaired")}</span>
                    )}
                  </td>
                  <td>
                    {pendingDevice?.device_id === d.device_id ? (
                      <span className="text-xs opacity-60">{t("pairings.waiting_hint")} ↓</span>
                    ) : (
                      <button
                        className="btn btn-primary btn-sm"
                        disabled={d.paired || startPair.isPending}
                        onClick={() => beginPair(d)}
                      >
                        {d.paired ? t("pairings.btn_paired") : t("pairings.btn_pair")}
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
            <h2 className="card-title">{t("pairings.waiting_hint")}</h2>
            <p className="text-sm text-base-content/60">
              {t("pairings.start_info", { name: pendingDevice.name })}
            </p>
            <div className="flex items-center gap-3 mt-2">
              <span
                className={`badge ${
                  remaining > 5 ? "badge-info" : "badge-warning"
                }`}
              >
                {remaining}s
              </span>
              <progress
                className="progress progress-primary w-full max-w-xs"
                value={remaining}
                max={PAIRING_TIMEOUT_SECS}
              />
              <button
                className="btn btn-ghost btn-sm"
                onClick={onCancel}
                disabled={rejectPair.isPending}
              >
                {t("pairings.waiting_cancel")}
              </button>
            </div>
            {startPair.error && (
              <div className="alert alert-error mt-2 text-sm">
                {(startPair.error as Error).message}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
