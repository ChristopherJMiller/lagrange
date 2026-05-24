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

# Make every writable runtime path owned by the nginx user (uid 101 in
# the official alpine image). Without this, dropping to USER nginx
# leaves /var/cache/nginx/* and /var/run unwritable — nginx tries to
# mkdir client_temp at startup and crashes with EACCES.
#
# /var/log/nginx/{access,error}.log are symlinks to /dev/std{out,err}
# in the upstream image; just chown the directory so a future config
# that writes to a real file doesn't trip.
#
# /tmp is mode 1777 by default; not touched here.
#
# Also: rewrite the PID path to a location the nginx user can write.
# The default /var/run/nginx.pid lives in /run on alpine and is
# root-owned; nginx as a non-root user can't replace it.
RUN set -eux; \
    chown -R nginx:nginx \
      /usr/share/nginx/html \
      /var/cache/nginx \
      /var/log/nginx; \
    mkdir -p /var/run/nginx; \
    chown -R nginx:nginx /var/run/nginx; \
    sed -i 's,^pid .*;,pid /var/run/nginx/nginx.pid;,' /etc/nginx/nginx.conf

EXPOSE 8080

USER nginx
CMD ["nginx", "-g", "daemon off;"]
