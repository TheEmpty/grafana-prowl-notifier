FROM rust:1.63-slim-buster

ENV BUILD_PACKAGES "gcc pkg-config libssl-dev"
ENV DEP_PACKAGES ""
ENV BINARY "grafana-prowl-notifier"

RUN apt-get update
RUN apt-get install -y ${BUILD_PACKAGES} ${DEP_PACKAGES}
RUN mkdir -p /code
COPY Cargo.toml /code/.
COPY src /code/src

RUN cd /code \
  && cargo build --release --verbose \
  && cp target/release/${BINARY} /opt/app \
  && rm -fr /src \
  && apt-get remove --purge ${BUILD_PACKAGES} \
  && apt-get clean

ENV RUST_LOG=trace
ENV RUST_BACKTRACE=1

ENTRYPOINT ["/opt/app"]