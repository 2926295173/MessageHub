"use client";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, wsUrl } from "@/lib/api";
import { useT } from "@/lib/i18n";

interface ConsoleEvent {
  kind: string;
  device_id: string;
  envelope_id: string;
  timestamp: number;
  summary: Record<string, unknown>;
}

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleTimeString();
}

function summarize(evt: ConsoleEvent, t: (k: any, v?: any) => string): string {
  switch (evt.kind) {
    case "console.hello":
      return t("live.ev_console_hello", {
        server: evt.summary.server ?? "daemon",
        version: evt.summary.version ?? "?",
      });
    case "device.hello":
      return t("live.ev_device_hello", {
        name: evt.summary.name ?? evt.device_id.slice(0, 8),
      });
    case "notification.received":
      return t("live.ev_notif", {
        app: evt.summary.app_name ?? evt.summary.package,
        title: evt.summary.title,
      });
    case "sms.received":
      return t("live.ev_sms", { addr: evt.summary.address, body: evt.summary.body });
    case "call.incoming":
      return t("live.ev_call_incoming");
    case "call.state":
      return t("live.ev_call_state");
    case "device.unpair":
      return t("live.ev_unpair");
    default:
      return evt.kind;
  }
}

export function LiveActivity() {
  const t = useT();
  const [events, setEvents] = useState<ConsoleEvent[]>([]);
  const [connected, setConnected] = useState(false);
  const dashboard = useQuery({
    queryKey: ["dashboard", "live"],
    queryFn: () => api.dashboard(),
    refetchInterval: 5_000,
  });

  useEffect(() => {
    let ws: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

    const connect = () => {
      ws = new WebSocket(`${wsUrl}/console`);
      ws.onopen = () => {
        setConnected(true);
      };
      ws.onmessage = (e) => {
        try {
          const evt = JSON.parse(e.data) as ConsoleEvent;
          setEvents((prev) => [evt, ...prev].slice(0, 30));
        } catch {
          // ignore
        }
      };
      ws.onclose = () => {
        setConnected(false);
        reconnectTimer = setTimeout(connect, 2000);
      };
      ws.onerror = () => {
        // onclose will fire; reconnect there
      };
    };
    connect();
    return () => {
      if (reconnectTimer) clearTimeout(reconnectTimer);
      if (ws) ws.close();
    };
  }, []);

  return (
    <div className="card bg-base-200">
      <div className="card-body p-4">
        <div className="flex items-center justify-between">
          <h2 className="card-title text-base">{t("live.title")}</h2>
          <span
            className={`badge badge-sm ${
              connected ? "badge-success" : "badge-ghost"
            }`}
          >
            {connected ? t("live.live") : t("live.disconnected")}
          </span>
        </div>
        <p className="text-xs opacity-60">
          {t("live.streamed_via")}
          {dashboard.data && (
            <span>
              {" "}{t("live.online_part", {
                online: dashboard.data.online_devices,
                paired: dashboard.data.paired_devices,
              })}
            </span>
          )}
        </p>
        <ul className="mt-2 max-h-72 space-y-1 overflow-y-auto text-sm">
          {events.length === 0 && (
            <li className="opacity-50">{t("live.empty")}</li>
          )}
          {events.map((e, i) => (
            <li key={i} className="flex items-baseline gap-2 truncate">
              <span className="font-mono text-[10px] opacity-50">{fmtTime(e.timestamp)}</span>
              <span className="truncate">{summarize(e, t)}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
