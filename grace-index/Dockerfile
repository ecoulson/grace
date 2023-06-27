FROM rust as builder
WORKDIR /app
COPY . .
RUN cargo install --profile release --path .

FROM debian:buster-slim as runner
COPY --from=builder /usr/local/cargo/bin/grace-index /usr/local/bin/grace-index
ENV ROCKET_ADDRESS=0.0.0.0
EXPOSE 8000
CMD ["grace-index"]
