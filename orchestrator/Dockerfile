FROM node:20-alpine

WORKDIR /app

# Install dependencies first (layer cache)
COPY package.json package-lock.json* ./
RUN npm install --frozen-lockfile

# Copy source and build
COPY tsconfig.json ./
COPY src/ ./src/
RUN npm run build

# Runtime — never bake .env into image
# Pass ALCHEMY_RPC_URL via docker run --env-file .env
CMD ["node", "dist/listener.js"]
