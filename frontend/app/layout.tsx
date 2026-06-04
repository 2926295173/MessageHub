import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'PhoneBridge',
  description: 'LAN-first Android phone bridge',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" data-theme="phonebridge">
      <body className="min-h-screen bg-base-100 text-base-content">
        {children}
      </body>
    </html>
  );
}
