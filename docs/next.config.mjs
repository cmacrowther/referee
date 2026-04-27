import { fileURLToPath } from 'url'
import path from 'path'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

/** @type {import('next').NextConfig} */
const nextConfig = {
  images: {
    unoptimized: true,
  },
  turbopack: {
    root: path.resolve(__dirname, '..'),
  },
  async redirects() {
    return [
      {
        source: '/integrate',
        destination: '/developers',
        permanent: true,
      },
    ]
  },
}

export default nextConfig
