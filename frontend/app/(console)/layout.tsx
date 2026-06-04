import Link from 'next/link';
import type { ReactNode } from 'react';

const NAV_ITEMS = [
  { href: '/dashboard/', label: 'Dashboard' },
  { href: '/devices/', label: 'Devices' },
  { href: '/pairings/', label: 'Pairings' },
  { href: '/notifications/', label: 'Notifications' },
  { href: '/sms/', label: 'SMS' },
  { href: '/calls/', label: 'Calls' },
  { href: '/settings/', label: 'Settings' },
];

export default function ConsoleLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen">
      <aside className="w-56 shrink-0 border-r border-base-300 bg-base-200 p-4">
        <div className="mb-6 px-2">
          <div className="text-lg font-semibold text-brand-500">PhoneBridge</div>
          <div className="text-xs text-base-content/60">Web Console</div>
        </div>
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
  );
}
