#!/bin/bash

set -ex

cargo fmt
cargo clippy -- -D warnings
cargo build
cargo build --release
cargo test --release

USER="theempty"
NAME="grafana-prowl-notifier"
TEST_REPO="192.168.7.7:5000"

docker build -t ${TEST_REPO}/${USER}/${NAME} .

# Publish to home lab- "bake"
docker push ${TEST_REPO}/${USER}/${NAME}
kubectl rollout restart deployment/${NAME}
sleep 45
kubectl logs -f -l app=${NAME}