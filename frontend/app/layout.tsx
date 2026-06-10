import type { Metadata } from 'next';
import './globals.css';
import { LocaleProvider } from '@/lib/i18n';

export const metadata: Metadata = {
  title: 'PhoneBridge',
  description: 'LAN-first, self-hosted bridge to manage multiple Android phones from a single desktop daemon',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" data-theme="phonebridge">
      <body className="min-h-screen bg-base-100 text-base-content">
        <LocaleProvider>
          {children}
        </LocaleProvider>
      </body>
    </html>
  );
}
