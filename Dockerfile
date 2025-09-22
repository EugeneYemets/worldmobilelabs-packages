# syntax=docker/dockerfile:1
FROM rust:1.80 as builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=builder /app/target/release/worldmobile-rs /app/app
EXPOSE 8000
ENV RUST_LOG=info
CMD ["/app/app"]
