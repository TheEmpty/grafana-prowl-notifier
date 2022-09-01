# Grafana Prowl Notifier

Provides a webhook for Grafana that sends Prowl notifications.

## Setup
* `docker run --rm -p 3333:3333 -v $(pwd):/config theempty/grafana-prowl-notifier /config/config.json`
* Add as webhook in Grafana notification policy.
* In the grafana policy, set max limit to `0` for unlimited.

## Ideas
* Grafana metadata that has the API keys
* Grafana metadata for priority
* Metrics for prometheus (queue size, retries, etc)
* Health check for something like kuma uptime
