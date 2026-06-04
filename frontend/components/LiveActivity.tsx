"use client";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, wsUrl } from "@/lib/api";

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

function summarize(evt: ConsoleEvent): string {
  switch (evt.kind) {
    case "console.hello":
      return `connected (${evt.summary.server ?? "daemon"} v${evt.summary.version ?? "?"})`;
    case "device.hello":
      return `device ${evt.summary.name ?? evt.device_id.slice(0, 8)} connected`;
    case "notification.received":
      return `[${evt.summary.app_name ?? evt.summary.package}] ${evt.summary.title}`;
    case "sms.received":
      return `SMS from ${evt.summary.address}: ${evt.summary.body}`;
    case "call.incoming":
    case "call.state":
      return `call ${evt.kind === "call.incoming" ? "incoming" : "state change"}`;
    case "device.unpair":
      return "device unpaired";
    default:
      return evt.kind;
  }
}

export function LiveActivity() {
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
          <h2 className="card-title text-base">Live activity</h2>
          <span
            className={`badge badge-sm ${
              connected ? "badge-success" : "badge-ghost"
            }`}
          >
            {connected ? "live" : "disconnected"}
          </span>
        </div>
        <p className="text-xs opacity-60">
          Streamed via <code>/ws/console</code>.{" "}
          {dashboard.data && (
            <span>
              {dashboard.data.online_devices}/{dashboard.data.paired_devices} online.
            </span>
          )}
        </p>
        <ul className="mt-2 max-h-72 space-y-1 overflow-y-auto text-sm">
          {events.length === 0 && (
            <li className="opacity-50">No events yet. Connect an Android device to see notifications, SMS, and calls stream in here.</li>
          )}
          {events.map((e, i) => (
            <li key={i} className="flex items-baseline gap-2 truncate">
              <span className="font-mono text-[10px] opacity-50">{fmtTime(e.timestamp)}</span>
              <span className="truncate">{summarize(e)}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
