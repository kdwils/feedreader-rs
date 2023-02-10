FROM rust:slim-buster as build
RUN USER=root apt-get update && apt-get --no-install-recommends install -y libssl-dev pkg-config openssl

RUN cargo new --bin feedreader
WORKDIR /feedreader

COPY Cargo.* ./

RUN cargo build --release && rm src/*.rs && rm target/release/deps/feedreader*

COPY . .
RUN cargo build --release

FROM debian:buster-slim

RUN USER=root apt-get update && apt-get --no-install-recommends install -y openssl ca-certificates

WORKDIR /feedreader
COPY --from=build --chown=1000:0 /feedreader/target/release/feedreader feedreader

EXPOSE 8080

CMD [ "./feedreader" ]