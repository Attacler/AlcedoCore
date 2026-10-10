FROM node:20-alpine

WORKDIR /app

# Copy plugin source
COPY . .

# Install dependencies
RUN npm install

EXPOSE 8080

CMD ["node", "server.js"]
