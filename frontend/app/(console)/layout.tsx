"use client";

import Link from 'next/link';
import { useState, type ReactNode } from 'react';
import { useQuery, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { api, type Dashboard, type Device, type Cert } from '@/lib/api';

const NAV_ITEMS = [
  { href: '/dashboard/', label: 'Dashboard' },
  { href: '/devices/', label: 'Devices' },
  { href: '/pairings/', label: 'Pairings' },
  { href: '/notifications/', label: 'Notifications' },
  { href: '/sms/', label: 'SMS' },
  { href: '/calls/', label: 'Calls' },
  { href: '/settings/', label: 'Settings' },
];

function ClientProviders({ children }: { children: ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 5_000,
            refetchOnWindowFocus: false,
            retry: 1,
          },
        },
      }),
  );
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function SidebarHeader() {
  const cert = useQuery({
    queryKey: ['cert'],
    queryFn: () => api.cert(),
    refetchInterval: 60_000,
  });
  const dashboard = useQuery({
    queryKey: ['dashboard', 'sidebar'],
    queryFn: () => api.dashboard(),
    refetchInterval: 10_000,
  });
  return (
    <div className="mb-6 px-2">
      <div className="text-lg font-semibold text-brand-500">PhoneBridge</div>
      <div className="text-xs text-base-content/60">Web Console</div>
      {cert.data && (
        <div className="mt-2 truncate rounded bg-base-300 p-2 text-xs">
          <div className="font-mono text-[10px] opacity-70">FP</div>
          <div className="truncate" title={cert.data.fingerprint}>
            {cert.data.fingerprint}
          </div>
        </div>
      )}
      {dashboard.data && (
        <div className="mt-2 text-xs text-base-content/60">
          {dashboard.data.online_devices}/{dashboard.data.paired_devices} online
        </div>
      )}
      <a
        href="/console/api-docs/"
        target="_blank"
        rel="noreferrer"
        className="link link-primary mt-3 block text-xs"
      >
        API docs (Swagger) →
      </a>
    </div>
  );
}

export default function ConsoleLayout({ children }: { children: ReactNode }) {
  return (
    <ClientProviders>
      <div className="flex min-h-screen">
        <aside className="w-56 shrink-0 border-r border-base-300 bg-base-200 p-4">
          <SidebarHeader />
          <nav className="flex flex-col gap-1">
            {NAV_ITEMS.map((item) => (
              <Link
                key={item.href}
                href={item.href}
                className="rounded px-3 py-2 text-sm hover:bg-base-300"
              >
                {item.label}
              </Link>
            ))}
          </nav>
        </aside>
        <main className="flex-1 p-6">{children}</main>
      </div>
    </ClientProviders>
  );
}
