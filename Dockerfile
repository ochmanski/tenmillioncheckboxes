# syntax = docker/dockerfile:1

ARG NODE_VERSION=20

FROM node:${NODE_VERSION}-alpine as base
LABEL fly_launch_runtime="Next.js"

# Prune the deps according to turbo
FROM base AS prune

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable
RUN pnpm install turbo --global
RUN apk add --no-cache libc6-compat
RUN apk update

WORKDIR /app
COPY . .
RUN turbo prune web --docker

# Install deps and build the app
FROM base AS builder
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable

WORKDIR /app

# First install the dependencies (as they change less often)
COPY --from=prune /app/out/json/ .
COPY --from=prune /app/out/pnpm-lock.yaml ./pnpm-lock.yaml
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile

COPY --from=prune /app/out/full/ .
COPY --from=prune /app/turbo.json turbo.json
RUN pnpm run build --filter=web...

# Run the app
# for some reason distroless image size is bigger than alpine
# FROM gcr.io/distroless/nodejs${NODE_VERSION}-debian12 as runner
FROM base as runner
ENV NODE_ENV production
WORKDIR /app

RUN addgroup --system --gid 1001 nodejs
RUN adduser --system --uid 1001 nextjs

RUN mkdir .next
RUN chown nextjs:nodejs .next

COPY --from=builder /app/apps/web/next.config.mjs .
COPY --from=builder /app/apps/web/package.json .


# Automatically leverage output traces to reduce image size
# https://nextjs.org/docs/advanced-features/output-file-tracing
COPY --from=builder --chown=nextjs:nodejs /app/apps/web/.next/standalone ./
COPY --from=builder --chown=nextjs:nodejs /app/apps/web/.next/static ./apps/web/.next/static

USER nextjs

EXPOSE 3000
ENV PORT 3000

CMD HOSTNAME="0.0.0.0" node apps/web/server.js
