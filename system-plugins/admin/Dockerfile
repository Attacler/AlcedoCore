FROM node:24-alpine AS builder
WORKDIR /build

# Copy the SDK into the build context
COPY sdk/alcedocore-sdk-node ./sdk/alcedocore-sdk-node

# Copy the app files into the admin directory
COPY system-plugins/admin/package.json ./system-plugins/admin/
COPY system-plugins/admin/manifest.json ./system-plugins/admin/
COPY system-plugins/admin/index.html ./system-plugins/admin/
COPY system-plugins/admin/vite.config.ts ./system-plugins/admin/
COPY system-plugins/admin/package-lock.json ./system-plugins/admin/
COPY system-plugins/admin/src ./system-plugins/admin/src

# Set the working directory to the admin folder
WORKDIR /build/system-plugins/admin

# Run npm install and build in the admin directory
RUN npm install
RUN npm run build

# Final stage
FROM scratch
WORKDIR /app

# Copy the built app files from the admin directory
COPY --from=builder /build/system-plugins/admin/public/ ./public/
COPY --from=builder /build/system-plugins/admin/manifest.json ./manifest.json