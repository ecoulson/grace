FROM rust as builder
WORKDIR /app
COPY . .
RUN cargo install --profile release --path .
ENV ROCKET_ADDRESS=0.0.0.0
EXPOSE 8000
CMD ["grace-index"]
