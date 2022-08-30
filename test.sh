#!/bin/bash

set -ex

cargo fmt
cargo clippy -- -D warnings
cargo build
cargo build --release

USER="theempty"
NAME="grafana-prowl-notifier"
TEST_REPO="192.168.7.7:5000"

sed -E -i .bak 's/ENV RUST_LOG=.+$/ENV RUST_LOG=trace/' Dockerfile
docker build -t ${TEST_REPO}/${USER}/${NAME} .

docker run --rm -v $(pwd):/config -p 3333:3333 ${TEST_REPO}/${USER}/${NAME} /config/config.json &
DOCKER_PID=$!

# "integ test"
sleep 5
curl http://localhost:3333 -d @test-packet.txt --header "Content-Type: application/json" --header "Expect:"
kill ${DOCKER_PID}

# Publish to home lab- "bake"
docker push ${TEST_REPO}/${USER}/${NAME}
kubectl rollout restart deployment/${NAME}
sleep 45
kubectl logs -f -l app=${NAME}