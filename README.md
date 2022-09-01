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

## Changelog

### 0.3.1
* Better error messages

### 0.3.0
* Persist fingerprints across reboots
* Breaking change: Config requires `fingerprints_file` entry.
* Another stab at cleaning up fingerprints over time.

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