import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'

declare const process: { cwd(): string }

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const target = env.VITE_API_TARGET || 'http://localhost:8080'

  return {
    // The SPA is served from /orbit/ behind the cluster ingress. Vite
    // rewrites absolute asset paths in index.html and the built JS so
    // chunks load as /orbit/assets/<hash>.js instead of /assets/....
    // The dev server also serves at this prefix, so `pnpm dev` opens at
    // http://localhost:5173/orbit/.
    base: '/orbit/',
    plugins: [react()],
    server: {
      port: 5173,
      // /v1 proxy is matched against the full request path (independent
      // of `base`), so the frontend's `fetch('/v1/...')` calls still
      // reach the admin service in dev.
      proxy: {
        '/v1': {
          target,
          changeOrigin: true,
        },
      },
    },
    build: {
      target: 'es2022',
      sourcemap: true,
    },
  }
})
