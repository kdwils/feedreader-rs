FROM --platform=$BUILDPLATFORM rust:slim-buster as build

RUN apt-get update && apt-get --no-install-recommends install -y     libssl-dev

RUN cargo new --bin feedreader
WORKDIR /feedreader

RUN cargo build --release && rm src/*.rs && rm target/release/deps/feedreader*

COPY . .

RUN cargo build --release

FROM debian:buster-slim

WORKDIR /feedreader
COPY --from=build /feedreader/target/release/feedreader feedreader

EXPOSE 8080

CMD [ "./feedreader" ]