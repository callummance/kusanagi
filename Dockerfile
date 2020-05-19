FROM rust:1.43 as build
WORKDIR /usr/src/kusanagi
COPY . .
RUN cargo install --path .

FROM debian:buster-slim
WORKDIR /
RUN apt-get update && apt-get install -y libssl-dev ca-certificates
COPY --from=build /usr/local/cargo/bin/kusanagi-bin /usr/local/bin/kusanagi
COPY ./phaseidentifiers /phaseidentifiers
ENTRYPOINT [ "kusanagi" ]