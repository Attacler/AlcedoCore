FROM node:20-alpine AS builder

WORKDIR /build

# Copy SDK and plugin source
COPY sdk/alcedocore-sdk-node ./alcedocore-sdk-node
COPY sample-plugins/hello-world-nuxt/ ./hello-world-nuxt/

# Build the SDK
RUN cd alcedocore-sdk-node && npm ci && npm run build

# Build the Nuxt plugin (alcedo-sdk is a local path dependency)
RUN cd hello-world-nuxt && npm install && npm run build

# Runtime stage - copy only the built SDK dist + the server output
FROM node:20-alpine
WORKDIR /app

COPY --from=builder /build/hello-world-nuxt/.output ./output/
COPY --from=builder /build/alcedocore-sdk-node /app/alcedocore-sdk-node
# Symlink so output server can import alcedo-sdk
RUN mkdir -p /app/output/node_modules && ln -s /app/alcedocore-sdk-node /app/output/node_modules/alcedocore-sdk-node
COPY sample-plugins/hello-world-nuxt/manifest.json /app/manifest.json

EXPOSE 8080
ENV NODE_ENV=production

CMD ["node", "output/server/index.mjs"]
