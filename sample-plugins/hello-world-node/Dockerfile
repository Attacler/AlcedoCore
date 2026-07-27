# Stage 1: Build the Alcedo SDK
FROM node:20-alpine AS sdk-builder

WORKDIR /build
COPY sdk/alcedo-sdk-node/ ./sdk/
RUN cd sdk && npm ci && npm run build

# Stage 2: Final image
FROM node:20-alpine

WORKDIR /app

# Copy the pre-built SDK
COPY --from=sdk-builder /build/sdk ./sdk/

# Copy plugin source
COPY sample-plugins/hello-world-node/package.json .
COPY sample-plugins/hello-world-node/server.js .
COPY sample-plugins/hello-world-node/manifest.json /app/manifest.json
COPY sample-plugins/hello-world-node/migrations/ ./migrations/
COPY sample-plugins/hello-world-node/pages/ ./pages/

# Install dependencies
RUN npm install

EXPOSE 8080

CMD ["node", "server.js"]
