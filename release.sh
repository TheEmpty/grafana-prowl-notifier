#!/bin/bash

set -ex

cargo clippy -- -D warnings
cargo build
cargo build --release

USER="theempty"
NAME="grafana-prowl-notifier"
VERSION=$(sed -E -n 's/^version = "([0-9\.]+)"/\1/p' Cargo.toml)
BUILDX="nostalgic_brattain"
PLATFORMS="linux/amd64,linux/arm64"

echo "Building for release, ${NAME}:${VERSION}"

TAGS=(
192.168.7.7:5000/${USER}/${NAME}
${USER}/${NAME}:latest
${USER}/${NAME}:${VERSION}
)

docker buildx build --builder ${BUILDX} $(join_tags) --push --platform=${PLATFORMS} .

kubectl rollout restart deployment/${NAME} || true
