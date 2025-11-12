FROM rust:1.82-alpine AS build

# Install build dependencies
RUN apk add --no-cache musl-dev pkgconfig openssl-dev

RUN mkdir /build
WORKDIR /build

# Copy Cargo files
COPY ./Cargo.toml .
COPY ./Cargo.lock* ./

# Copy source code
COPY ./src ./src

# Build the application
RUN cargo build --release

FROM alpine
WORKDIR /app

# Copy the binary
COPY --from=build /build/target/release/tondi-graph-inspector-processing /app/processing

# Copy database migrations
RUN mkdir -p /app/database/migrations
COPY ./database/migrations/ /app/database/migrations/
