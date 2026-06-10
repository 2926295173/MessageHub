"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type Device } from "@/lib/api";
import { useT } from "@/lib/i18n";

function fmtTime(t: number): string {
  if (!t) return "—";
  return new Date(t * 1000).toLocaleString();
}

/**
 * Devices — operational view: who is connected, who's paired, when
 * were they last seen. The long-term credentials (device id, public
 * key) are intentionally NOT shown on this page; they live on the
 * About page so that a glance at the device list doesn't surface
 * the secrets needed to impersonate a paired phone.
 */
export default function DevicesPage() {
  const t = useT();
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
  const remove = useMutation({
    mutationFn: (id: string) => api.removeDevice(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
  });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("devices.title")}</h1>
        <p className="text-sm text-base-content/60">
          {t("devices.subtitle", { n: dashboard.data?.online_devices ?? 0 })}
        </p>
      </header>

      <div className="card bg-base-200">
        <div className="card-body p-0">
          <table className="table">
            <thead>
              <tr>
                <th>{t("devices.col.name")}</th>
                <th>{t("devices.col.paired")}</th>
                <th>{t("devices.col.last_seen")}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {devices.isLoading && (
                <tr>
                  <td colSpan={4} className="text-center text-sm opacity-60">
                    {t("devices.loading")}
                  </td>
                </tr>
              )}
              {devices.data?.devices.length === 0 && (
                <tr>
                  <td colSpan={4} className="text-center text-sm opacity-60">
                    {t("devices.empty")}
                  </td>
                </tr>
              )}
              {devices.data?.devices.map((d: Device) => (
                <tr key={d.id}>
                  <td>{d.name}</td>
                  <td>
                    {d.paired ? (
                      <span className="badge badge-success badge-sm">
                        {t("devices.badge_paired")}
                      </span>
                    ) : (
                      <span className="badge badge-ghost badge-sm">
                        {t("devices.badge_discovered")}
                      </span>
                    )}
                  </td>
                  <td className="text-xs opacity-60">{fmtTime(d.last_seen)}</td>
                  <td>
                    <button
                      className="btn btn-ghost btn-xs text-error"
                      disabled={remove.isPending}
                      onClick={() => {
                        if (confirm(t("devices.unpair_confirm", { name: d.name }))) {
                          remove.mutate(d.device_id);
                        }
                      }}
                    >
                      {t("devices.unpair_btn")}
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
