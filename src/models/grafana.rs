use derive_getters::Getters;
use prowl::Priority;
use serde::Deserialize;

#[derive(Deserialize, Getters)]
pub(crate) struct Message {
    alerts: Vec<Alert>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Getters)]
pub(crate) struct Alert {
    status: String,
    labels: Label,
    annotations: Annotation,
    #[serde(rename = "generatorURL")]
    generator_url: String,
    fingerprint: String,
}

#[derive(Deserialize, Getters)]
pub(crate) struct Label {
    alertname: String,
}

#[derive(Deserialize, Getters)]
pub(crate) struct Annotation {
    summary: String,
}

impl Alert {
    pub(crate) fn get_priority(&self) -> Priority {
        if self.status() == "firing" {
            let alertname = &self.labels().alertname();
            if alertname.starts_with("[critical]") || alertname.starts_with("[CRIT]") {
                Priority::Emergency
            } else if alertname.starts_with("[high]") || alertname.starts_with("[HIGH]") {
                Priority::High
            } else {
                Priority::Normal
            }
        } else {
            Priority::VeryLow
        }
    }
}

#[cfg(test)]
mod test {
    use crate::models::grafana::Alert;
    use prowl::Priority;

    #[test]
    fn no_prefix() {
        let firing: Alert = serde_json::from_str(&crate::test::consts::create_firing_alert())
            .expect("Failed to load default, firing alert");
        let resolved: Alert = serde_json::from_str(&crate::test::consts::create_resolved_alert())
            .expect("Failed to load default, resolved alert");
        assert_eq!(firing.get_priority(), Priority::Normal);
        assert_eq!(resolved.get_priority(), Priority::VeryLow);
    }

    #[test]
    fn critical_prefix() {
        let firing: Alert = serde_json::from_str(
            &crate::test::consts::create_firing_alert_with_prefix("[critical] "),
        )
        .expect("Failed to load default, firing alert");
        let resolved: Alert = serde_json::from_str(
            &crate::test::consts::create_resolved_alert_with_prefix("[critical] "),
        )
        .expect("Failed to load default, resolved alert");
        assert_eq!(firing.get_priority(), Priority::Emergency);
        assert_eq!(resolved.get_priority(), Priority::VeryLow);

        let firing: Alert = serde_json::from_str(
            &crate::test::consts::create_firing_alert_with_prefix("[CRIT] "),
        )
        .expect("Failed to load default, firing alert");
        let resolved: Alert = serde_json::from_str(
            &crate::test::consts::create_resolved_alert_with_prefix("[CRIT] "),
        )
        .expect("Failed to load default, resolved alert");
        assert_eq!(firing.get_priority(), Priority::Emergency);
        assert_eq!(resolved.get_priority(), Priority::VeryLow);
    }

    #[test]
    fn high_prefix() {
        let firing: Alert = serde_json::from_str(
            &crate::test::consts::create_firing_alert_with_prefix("[high] "),
        )
        .expect("Failed to load default, firing alert");
        let resolved: Alert = serde_json::from_str(
            &crate::test::consts::create_resolved_alert_with_prefix("[high] "),
        )
        .expect("Failed to load default, resolved alert");
        assert_eq!(firing.get_priority(), Priority::High);
        assert_eq!(resolved.get_priority(), Priority::VeryLow);

        let firing: Alert = serde_json::from_str(
            &crate::test::consts::create_firing_alert_with_prefix("[HIGH] "),
        )
        .expect("Failed to load default, firing alert");
        let resolved: Alert = serde_json::from_str(
            &crate::test::consts::create_resolved_alert_with_prefix("[HIGH] "),
        )
        .expect("Failed to load default, resolved alert");
        assert_eq!(firing.get_priority(), Priority::High);
        assert_eq!(resolved.get_priority(), Priority::VeryLow);
    }
}
