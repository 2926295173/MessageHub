/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'export',
  basePath: '/console',
  trailingSlash: true,
  reactStrictMode: true,
  images: {
    unoptimized: true,
  },
  typescript: {
    ignoreBuildErrors: false,
  },
};

export default nextConfig;
