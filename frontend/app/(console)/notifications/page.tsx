"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, type Notification } from "@/lib/api";
import { useT } from "@/lib/i18n";

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

export default function NotificationsPage() {
  const t = useT();
  const qc = useQueryClient();
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [packageFilter, setPackageFilter] = useState("");
  const notifications = useQuery({
    queryKey: ["notifications", "page", unreadOnly, packageFilter],
    queryFn: () =>
      api.notifications({
        limit: 200,
        unread_only: unreadOnly,
        package: packageFilter || undefined,
      }),
    refetchInterval: 5_000,
  });
  const stats = useQuery({
    queryKey: ["notifications", "stats"],
    queryFn: () => api.notificationsStats(),
    refetchInterval: 10_000,
  });
  const markRead = useMutation({
    mutationFn: ({ device_id, id }: { device_id: string; id: string }) =>
      api.markNotificationRead(device_id, id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["notifications"] }),
  });
  const dismiss = useMutation({
    mutationFn: ({ device_id, id }: { device_id: string; id: string }) =>
      api.dismissNotification(device_id, id),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ["notifications"] });
      // Soft feedback in the console; the device side does the work.
      // eslint-disable-next-line no-console
      console.log(`[phonebridge] dismissed ${data.id} (broadcast=${data.broadcast})`);
    },
  });

  return (
    <div className="space-y-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("notif.title")}</h1>
          <p className="text-sm text-base-content/60">
            {t("notif.subtitle")}
          </p>
        </div>
        <div className="text-sm">
          {stats.data && (
            <span className="badge badge-lg">
              {stats.data.unread} / {stats.data.total}
            </span>
          )}
        </div>
      </header>

      <section className="flex items-center gap-3">
        <label className="label cursor-pointer gap-2">
          <input
            type="checkbox"
            className="checkbox checkbox-sm"
            checked={unreadOnly}
            onChange={(e) => setUnreadOnly(e.target.checked)}
          />
          <span className="text-sm">{t("notif.unread_only")}</span>
        </label>
        <input
          type="text"
          className="input input-bordered input-sm"
          placeholder={t("notif.filter_placeholder")}
          value={packageFilter}
          onChange={(e) => setPackageFilter(e.target.value)}
        />
      </section>

      {stats.data && stats.data.by_package.length > 0 && (
        <section className="flex flex-wrap gap-2">
          {stats.data.by_package.slice(0, 12).map((p) => (
            <button
              key={p.package}
              className="badge badge-outline cursor-pointer"
              onClick={() => setPackageFilter(p.package)}
            >
              {p.package} <span className="ml-1 opacity-60">{p.count}</span>
            </button>
          ))}
        </section>
      )}

      <div className="space-y-2">
        {notifications.isLoading && <div className="text-sm opacity-60">{t("notif.loading")}</div>}
        {notifications.data?.notifications.length === 0 && (
          <div className="text-sm opacity-60">{t("notif.empty")}</div>
        )}
        {notifications.data?.notifications.map((n: Notification) => (
          <div
            key={`${n.device_id}-${n.id}`}
            className={`card bg-base-200 ${n.read ? "opacity-60" : ""}`}
          >
            <div className="card-body p-4">
              <div className="flex items-start justify-between">
                <div className="flex-1">
                  <div className="flex items-center gap-2 text-xs opacity-60">
                    <span className="badge badge-ghost badge-sm">
                      {n.app_name ?? n.package_name}
                    </span>
                    <span>{fmtTime(n.posted_at)}</span>
                    {n.is_sensitive && (
                      <span className="badge badge-warning badge-sm">{t("notif.sensitive")}</span>
                    )}
                  </div>
                  <div className="mt-1 font-medium">{n.title}</div>
                  <div className="text-sm opacity-80">
                    {n.is_sensitive ? <em>{t("notif.hidden")}</em> : n.content}
                  </div>
                </div>
                <div className="flex flex-col gap-1 items-end">
                  {!n.read && (
                    <button
                      className="btn btn-ghost btn-xs"
                      onClick={() =>
                        markRead.mutate({ device_id: n.device_id, id: n.id })
                      }
                      disabled={markRead.isPending}
                    >
                      {t("notif.mark_read")}
                    </button>
                  )}
                  <button
                    className="btn btn-error btn-xs"
                    onClick={() =>
                      dismiss.mutate({ device_id: n.device_id, id: n.id })
                    }
                    disabled={dismiss.isPending}
                    title={t("notif.dismiss_title")}
                  >
                    {dismiss.isPending ? t("notif.dismissing") : t("notif.dismiss")}
                  </button>
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
