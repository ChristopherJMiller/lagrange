# syntax=docker/dockerfile:1.7

# ── build ─────────────────────────────────────────────────────────────
FROM node:22-alpine AS build
WORKDIR /app

# install deps first for layer caching
COPY package.json pnpm-lock.yaml* package-lock.json* ./
# Pin pnpm to v9 to match the lockfile (the nix devShell uses pnpm_9).
# pnpm 10 changed --frozen-lockfile semantics around ignored-build-scripts
# and rejects installs whose lockfile was emitted by 9.x.
RUN if [ -f pnpm-lock.yaml ]; then \
      npm install -g pnpm@9 && pnpm install --frozen-lockfile; \
    elif [ -f package-lock.json ]; then \
      npm ci; \
    else \
      npm install; \
    fi

COPY . .
RUN npm run build

# ── serve ─────────────────────────────────────────────────────────────
FROM nginx:1.27-alpine AS runtime

# nginx config: SPA fallback, no auth, no /v1 proxy (ingress handles that)
COPY nginx.conf /etc/nginx/conf.d/default.conf

COPY --from=build /app/dist /usr/share/nginx/html

EXPOSE 8080

# nginx user, no root
RUN sed -i 's,listen       80;,listen 8080;,g' /etc/nginx/conf.d/default.conf 2>/dev/null || true \
  && chown -R nginx:nginx /usr/share/nginx/html

USER nginx
CMD ["nginx", "-g", "daemon off;"]
