FROM --platform=$BUILDPLATFORM rust:slim-buster as build

RUN apt-get update && apt-get --no-install-recommends install -y pkg-config libssl-dev

WORKDIR /feedreader

COPY . .

RUN cargo build --release

FROM debian:buster-slim

WORKDIR /feedreader
COPY --from=build /feedreader/target/release/feedreader feedreader

EXPOSE 8080

CMD [ "./feedreader" ]