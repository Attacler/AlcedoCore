FROM node:22-alpine AS builder

WORKDIR /build

# Copy plugin source
COPY sample-plugins/hello-world-nuxt/ ./hello-world-nuxt/

# Build the Nuxt plugin (SDK is pulled from npm)
# NUXT_BASE_URL=/nuxt-test so the app is built for the /nuxt-test route alias.
RUN cd hello-world-nuxt && NUXT_BASE_URL=/nuxt-test npm install --no-audit --no-fund && NUXT_BASE_URL=/nuxt-test npm run build

# Runtime stage
FROM node:22-alpine
WORKDIR /app

COPY --from=builder /build/hello-world-nuxt/.output ./output/
COPY sample-plugins/hello-world-nuxt/manifest.json /app/manifest.json

EXPOSE 8080
ENV NODE_ENV=production

CMD ["node", "output/server/index.mjs"]
