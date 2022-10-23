FROM rust:alpine

# Packages
ENV BUILD_PACKAGES "pkgconfig"
ENV DEP_PACKAGES "gcc openssl-dev musl-dev"
RUN apk add --no-cache ${BUILD_PACKAGES} ${DEP_PACKAGES}

# Code
RUN mkdir -p /code
COPY Cargo.toml /code/.
COPY src /code/src

# Build vars
ENV BINARY "grafana-prowl-notifier"
# Believe this requirement stems from reqwest
ENV RUSTFLAGS="-Ctarget-feature=-crt-static"

# Compile && Cleanup
RUN cd /code \
  && cargo build --release --verbose \
  && cp target/release/${BINARY} /opt/app \
  && rm -fr /code \
  && apk --purge del ${BUILD_PACKAGES}

# Runtime env
ENV RUST_LOG=debug
ENV RUST_BACKTRACE=1

ENTRYPOINT ["/opt/app"]
