# Grafana Prowl Notifier

Provides a webhook for Grafana that sends Prowl notifications.

## Setup
* `docker run --rm -p 3333:3333 -v $(pwd):/config theempty/grafana-prowl-notifier /config/config.json`
* Add as webhook in Grafana notification policy with the path of `/webhooks/grafana` ex: `http://127.0.0.1/webhooks/grafana`
* In the grafana policy, set max limit to `0` for unlimited.

## Scaling Considerations
Each alarm recieved will hold a "fingerprint" structure.
It is not released and will be reloaded on restart.
Therefore the memory scales with the number of notifications.

## Ideas
* Grafana metadata that has the API keys
* Grafana metadata for priority
* Metrics for prometheus (queue size, retries, etc)
* Health check for something like kuma uptime

## Dev notes
* lame integ test: `curl -v http://localhost:3333 -d @test-packet.txt --header "Content-Type: application/json" --header "Expect:"`

## Changelog

### 0.5.0 (WIP)
* Store more info on fingerprints
* Change webhook URL from `/` to `/webhooks/grafana`.

### 0.4.1
* Save fingerprints after re-alerting.

### 0.4.0
* Bugfix: Do not hang on reading TCP stream until connection is dropped.
* Breaking change: No migrate function for adding new mandatory field `last_alerted` since I'm being lazy and am okay with re-alerting.
* Add support for re-alerting if alarm is still alerting after `alert_every_minutes` config.
* Unit testing
* Add `test_mode` config to prevent sending notifications.

### 0.3.3
* Fingerprints to own data structure.
* Auto-migrate to new data structure.

### 0.3.2
* Bugfix: Don't cleanup fingerprints if not seen in request.
* Add note: Longer-term need to understand how fingerprints scale.

### 0.3.1
* Better error messages
* Known issue: drops fingerprints from cache too quickly.

### 0.3.0
* Persist fingerprints across reboots
* Breaking change: Config requires `fingerprints_file` entry.
* Another stab at cleaning up fingerprints over time.
* Known issue: drops fingerprints from cache too quickly.

### 0.2.3
* Add `wait_secs_between_notifications` to config, only used in batches.
* Bug-fix: just because an alert is resolved doesn't mean it won't be called again.

### 0.2.2
* Move to Alpine for docker

### 0.2.1
* Move to Tokio channels for queueing and sending notifications
* General refactor

## Example Kube setup
```
apiVersion: apps/v1
kind: Deployment
metadata:
  name: grafana-prowl-notifier
spec:
  selector:
    matchLabels:
      app: grafana-prowl-notifier
  template:
    metadata:
      labels:
        app: grafana-prowl-notifier
    spec:
      securityContext:
        fsGroup: 0
        fsGroupChangePolicy: "OnRootMismatch"
      restartPolicy: Always
      volumes:
      - name: notifier-config
        configMap:
          name: grafana-prowl-notifier-config
      - name: notifier-data
        persistentVolumeClaim:
          claimName: grafana-prowl-notifier-data
      containers:
      - name: grafana-prowl-notifier
        image: theempty/grafana-prowl-notifier:latest
        resources:
          limits:
            cpu: "1m"
            memory: "64Mi"
        volumeMounts:
          - name: notifier-config
            mountPath: /etc/grafana-prowl-notifier
          - name: notifier-data
            mountPath: /var/grafana-prowl-notifier
        args:
          - /etc/grafana-prowl-notifier/config.json
        ports:
        - containerPort: 3333

---

apiVersion: v1
kind: ConfigMap
metadata:
  name: grafana-prowl-notifier-config
data:
  config.json: |
    {
        "app_name": "My Kluster",
        "fingerprints_file": "/var/grafana-prowl-notifier/fingerprints.json",
        "prowl_api_keys": [
            "YOURS HERE"
        ]
    }

---

apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: grafana-prowl-notifier-data
spec:
  accessModes:
    - ReadWriteOnce
  storageClassName: longhorn
  resources:
    requests:
      storage: 50M

---

apiVersion: v1
kind: Service
metadata:
  name: grafana-prowl-notifier
spec:
  type: LoadBalancer
  loadBalancerIP: 192.168.7.19
  selector:
    app: grafana-prowl-notifier
  ports:
  - port: 80
    targetPort: 3333
status:
  loadBalancer: {}
```