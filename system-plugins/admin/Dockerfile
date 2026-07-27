FROM node:24-alpine AS sdk-builder
WORKDIR /src
COPY sdk/alcedo-sdk-node/ ./
RUN npm install && npm run build

FROM node:24-alpine AS builder
WORKDIR /build
COPY --from=sdk-builder /src/ /sdk/alcedo-sdk-node/
COPY system-plugins/admin/package.json ./
RUN npm install
COPY system-plugins/admin/ .
RUN npm run build

FROM scratch
COPY --from=builder /build/public/ /app/public/
COPY --from=builder /build/manifest.json /app/manifest.json
