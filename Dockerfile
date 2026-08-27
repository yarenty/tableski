# Build:  docker build -t tableski .
# Run:    docker run --rm -p 8080:8080 tableski
FROM rust:1.85 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/tableski /usr/local/bin/
COPY fixtures/sample.csv /data/sample.csv
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/tableski"]
CMD ["--csv", "/data/sample.csv", "--bind", "0.0.0.0:8080"]
