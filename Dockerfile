FROM rust:slim-buster as build

WORKDIR /feedreader

COPY . .

RUN cargo build --release

FROM debian:buster-slim

WORKDIR /feedreader
COPY --from=build /feedreader/target/release/feedreader feedreader

EXPOSE 8080

CMD [ "./feedreader" ]