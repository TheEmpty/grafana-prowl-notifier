# Grafana Prowl Notifier

Provides a webhook for Grafana that sends Prowl notifications.

## Setup
* Create a config.json, see `config.example.json` or below.
* `docker run --rm -p 3333:3333 -v $(pwd):/config theempty/grafana-prowl-notifier /config/config.json`
* Add as webhook in Grafana notification policy with the path of `/webhooks/grafana` ex: `http://127.0.0.1/webhooks/grafana`
* In the grafana policy, set max limit to `0` for unlimited.

## config.json
Possible fields:

### prowl_api_keys `[string]` - REQUIRED
The API keys that devices that you want to notify for alarms.

### fingerprints_file `string` - REQUIRED
Where to store the persistent file of what alarms have already
been notified, when, and other meta-data.

### app_name `string` default: "Grafana"
The name that appears on the prowl notification.
This is useful if you have multiple instances of grafana and
grafana-prowl-notifier, so you know which host is alarming.

### linear_retry_secs `int` default: 60
How long to wait (in seconds) before retrying a request to
the Prowl API.

### bind_host `string` default: "0.0.0.0:3333"
The interface and port to bind the HTTP service to.

### alert_every_minutes `int` - optional
Re-alert every X minutes if an alarm is not yet resolved.
Example: realert every 1440 minutes (24hr) if I have not resolved the alarm.
Can be used with `realert_cron` if desired.

### realert_cron `string` - optional
Use a UTC crontab to specify when re-alerting should happen.
Example: `0 0,16 * * *` to alert me at 9am and 5pm PST with alarms that are still active.
Can be used with `alert_every_minutes` if desired.

### test_mode `boolean` - optional
Set to `true` to prevent calls from the Prowl API. Notifications will just
be dequeued without any work.

## Scaling Considerations
Each alarm recieved will hold a "fingerprint" structure.
It is not released and will be reloaded on restart.
Therefore the memory scales with the number of notifications.
Optimizations are possible, but currently unneeded.

## Ideas
* Grafana metadata that has the API keys
* Grafana metadata for priority
* Metrics for prometheus (queue size, retries, etc)
* Health check for something like kuma uptime
* Next major version change `alert_every_minutes` to `realert_every_minutes`

## Dev notes
* lame integ test: `curl -v http://localhost:3333 -d @test-packet.txt --header "Content-Type: application/json" --header "Expect:"`

## Changelog

### 0.6.0
* Breaking: removed option `wait_secs_between_notifications`
* Move to prowl-queue

### 0.5.3
* Add cron parser for realerting :)

### 0.5.2
* Add ability to delete fingerprints.

### 0.5.1
* Added root HTML page that shows fingerprints.

### 0.5.0
* Store more info on fingerprints
* Change webhook URL from `/` to `/webhooks/grafana`.
* Refactor to web service

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