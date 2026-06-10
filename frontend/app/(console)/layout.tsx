"use client";

import Link from "next/link";
import { useState, type ReactNode } from "react";
import { keepPreviousData, useQuery, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useT, type TranslationKey } from "@/lib/i18n";

function ClientProviders({ children }: { children: ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 5_000,
            refetchOnWindowFocus: false,
            retry: 1,
            // Keep the previous value visible while a new
            // refetch is in flight. Without this the sidebar's
            // "X/Y online" count momentarily disappears on
            // every navigation, because the parent layout
            // remounts and the query starts from `undefined`
            // until the first response lands.
            placeholderData: keepPreviousData,
          },
        },
      }),
  );
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

/**
 * Sidebar — fixed-position navigation rail. Uses
 * `sticky top-0 h-screen` so the rail stays put when the main
 * content scrolls, but still scrolls along with the page on
 * very tall viewports (a hard `fixed` would have made it
 * lose its place at the bottom of the document).
 *
 * Intentionally dumb: title, online count, and nav. Anything
 * sensitive (fingerprint, pubkey, device id) lives on the
 * About page; anything controllable (language toggle) lives
 * on Settings. A quick glance at the rail is not a
 * credentials leak.
 */
function SidebarHeader() {
  const t = useT();
  const dashboard = useQuery({
    queryKey: ["dashboard", "sidebar"],
    queryFn: () => api.dashboard(),
    // Slow poll — the on-page dashboard does the heavy
    // lifting. This is just for the rail's "X/Y online"
    // count. Without `keepPreviousData` (set in the
    // QueryClient defaults above) the count vanishes for a
    // frame on every navigation; with it, the old value
    // stays on screen until the fresh data lands.
    refetchInterval: 10_000,
  });
  // Always render the count so the layout doesn't shift
  // when the data is undefined for a frame.
  const online = dashboard.data?.online_devices;
  const paired = dashboard.data?.paired_devices;
  return (
    <div className="mb-6 px-2">
      <div className="text-lg font-semibold text-brand-500">PhoneBridge</div>
      <div className="text-xs text-base-content/60">{t("layout.console_subtitle")}</div>
      <div className="mt-2 text-xs text-base-content/60 min-h-[1.25rem]">
        {online == null || paired == null
          ? "\u00A0" // non-breaking space keeps the row's height
          : t("layout.online_count", { online, paired })}
      </div>
    </div>
  );
}

const NAV_ITEMS: Array<{ href: string; key: TranslationKey }> = [
  { href: "/dashboard/", key: "nav.dashboard" },
  { href: "/devices/", key: "nav.devices" },
  { href: "/pairings/", key: "nav.pairings" },
  { href: "/notifications/", key: "nav.notifications" },
  { href: "/sms/", key: "nav.sms" },
  { href: "/calls/", key: "nav.calls" },
  { href: "/settings/", key: "nav.settings" },
  { href: "/about/", key: "nav.about" },
];

export default function ConsoleLayout({ children }: { children: ReactNode }) {
  const t = useT();
  return (
    <ClientProviders>
      <div className="flex min-h-screen">
        <aside
          className="
            sticky top-0
            h-screen w-56 shrink-0
            border-r border-base-300 bg-base-200 p-4
            overflow-y-auto
          "
        >
          <SidebarHeader />
          <nav className="flex flex-col gap-1">
            {NAV_ITEMS.map((item) => (
              <Link
                key={item.href}
                href={item.href}
                className="rounded px-3 py-2 text-sm hover:bg-base-300"
              >
                {t(item.key)}
              </Link>
            ))}
          </nav>
        </aside>
        <main className="flex-1 p-6 min-w-0">{children}</main>
      </div>
    </ClientProviders>
  );
}
