"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api, type Device, type Sms, type SmsConversation } from "@/lib/api";
import { useT } from "@/lib/i18n";

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

export default function SmsPage() {
  const t = useT();
  const qc = useQueryClient();
  const [deviceId, setDeviceId] = useState<string | "">("");
  const [selected, setSelected] = useState<string | null>(null);
  const [compose, setCompose] = useState({ to: "", body: "" });
  const [sendError, setSendError] = useState<string | null>(null);

  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: () => api.devices(),
    refetchInterval: 30_000,
  });
  const conversations = useQuery({
    queryKey: ["sms", "conversations", deviceId],
    queryFn: () => api.smsConversations({ device_id: deviceId || undefined }),
    refetchInterval: 5_000,
  });
  const messages = useQuery({
    queryKey: ["sms", "messages", deviceId, selected],
    queryFn: () =>
      api.sms({
        device_id: deviceId || undefined,
        phone_number: selected ?? undefined,
        limit: 200,
      }),
    refetchInterval: 3_000,
    enabled: !!selected,
  });

  const sendSms = useMutation({
    mutationFn: (req: { device_id: string; to: string; body: string }) => api.sendSms(req),
    onSuccess: () => {
      setCompose({ to: "", body: "" });
      setSendError(null);
      qc.invalidateQueries({ queryKey: ["sms"] });
    },
    onError: (e) => setSendError((e as Error).message),
  });

  const conversationsById = useMemo(() => {
    const m = new Map<string, SmsConversation>();
    conversations.data?.conversations.forEach((c) => m.set(c.address, c));
    return m;
  }, [conversations.data]);

  return (
    <div className="space-y-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("sms.title")}</h1>
          <p className="text-sm text-base-content/60">{t("sms.subtitle")}</p>
        </div>
        <select
          className="select select-bordered select-sm"
          value={deviceId}
          onChange={(e) => {
            setDeviceId(e.target.value);
            setSelected(null);
          }}
        >
          <option value="">{t("sms.all_devices")}</option>
          {devices.data?.devices.map((d: Device) => (
            <option key={d.device_id} value={d.device_id}>
              {d.name}
            </option>
          ))}
        </select>
      </header>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="card bg-base-200">
          <div className="card-body p-0">
            <div className="border-b border-base-300 p-3 text-sm font-semibold">
              {t("sms.conversations", { n: conversations.data?.conversations.length ?? 0 })}
            </div>
            <ul className="max-h-[60vh] overflow-y-auto">
              {conversations.data?.conversations.length === 0 && (
                <li className="p-4 text-sm opacity-60">{t("sms.no_conversations")}</li>
              )}
              {conversations.data?.conversations.map((c) => (
                <li key={c.address}>
                  <button
                    onClick={() => setSelected(c.address)}
                    className={`flex w-full items-center justify-between border-b border-base-300 p-3 text-left hover:bg-base-300 ${
                      selected === c.address ? "bg-base-300" : ""
                    }`}
                  >
                    <div>
                      <div className="text-sm font-medium">{c.address}</div>
                      <div className="text-xs opacity-50">{fmtTime(c.last_timestamp)}</div>
                    </div>
                    <span className="badge badge-sm">{c.count}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>

        <div className="card bg-base-200 lg:col-span-2">
          <div className="card-body p-0">
            <div className="border-b border-base-300 p-3 text-sm font-semibold">
              {selected ? `${t("sms.title")} · ${selected}` : t("sms.pick_conversation")}
            </div>
            <ul className="max-h-[50vh] space-y-1 overflow-y-auto p-3">
              {!selected && (
                <li className="text-sm opacity-60">{t("sms.no_conversation_selected")}</li>
              )}
              {messages.data?.messages.length === 0 && selected && (
                <li className="text-sm opacity-60">{t("sms.no_messages")}</li>
              )}
              {messages.data?.messages.map((m: Sms) => (
                <li
                  key={m.id}
                  className={`max-w-[80%] rounded p-2 text-sm ${
                    m.direction === "out"
                      ? "ml-auto bg-brand-700/40 text-right"
                      : "mr-auto bg-base-300"
                  }`}
                >
                  <div className="text-xs opacity-60">{fmtTime(m.timestamp)}</div>
                  <div className="whitespace-pre-wrap">{m.body}</div>
                </li>
              ))}
            </ul>
            {selected && (
              <form
                className="flex flex-col gap-2 border-t border-base-300 p-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (!deviceId) {
                    setSendError(t("sms.select_device_first"));
                    return;
                  }
                  sendSms.mutate({
                    device_id: deviceId,
                    to: compose.to || selected,
                    body: compose.body,
                  });
                }}
              >
                <div className="flex gap-2">
                  <input
                    type="tel"
                    className="input input-bordered input-sm flex-1"
                    placeholder={t("sms.placeholder_to")}
                    value={compose.to || selected || ""}
                    onChange={(e) => setCompose((c) => ({ ...c, to: e.target.value }))}
                  />
                </div>
                <textarea
                  className="textarea textarea-bordered"
                  rows={2}
                  placeholder={t("sms.placeholder_body")}
                  value={compose.body}
                  onChange={(e) => setCompose((c) => ({ ...c, body: e.target.value }))}
                />
                <div className="flex items-center justify-between">
                  <span className="text-xs opacity-60">
                    {t("sms.send_via", { name: devices.data?.devices.find((d) => d.device_id === deviceId)?.name ?? "—" })}
                  </span>
                  <button
                    type="submit"
                    className="btn btn-primary btn-sm"
                    disabled={!compose.body || sendSms.isPending}
                  >
                    {t("sms.send")}
                  </button>
                </div>
                {sendError && <div className="alert alert-error text-xs">{sendError}</div>}
              </form>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
