FROM rust:1.63-slim-buster

RUN apt-get update
RUN apt-get install -y gcc libssl-dev pkg-config
RUN mkdir -p /code
COPY Cargo.toml /code/.
COPY src /code/src

RUN cd /code \
  && cargo build --release --verbose \
  && cp target/release/grafana-prowl-notifier /opt \
  && rm -fr /src

ENV RUST_LOG=debug
ENV RUST_BACKTRACE=1

ENTRYPOINT ["/opt/grafana-prowl-notifier"]
