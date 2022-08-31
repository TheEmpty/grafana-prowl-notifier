use derive_getters::Getters;
use serde::Deserialize;

#[derive(Deserialize, Getters)]
pub struct Message {
    alerts: Vec<Alert>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Getters)]
pub struct Alert {
    status: String,
    labels: Label,
    annotations: Annotation,
    #[serde(rename = "generatorURL")]
    generator_url: String,
    fingerprint: String,
}

#[derive(Deserialize, Getters)]
pub struct Label {
    alertname: String,
}

#[derive(Deserialize, Getters)]
pub struct Annotation {
    summary: String,
}

impl Alert {
    pub fn get_priority(&self) -> prowl::Priority {
        if self.status() == "firing" {
            let alertname = &self.labels().alertname();
            if alertname.starts_with("[critical]") || alertname.starts_with("[CRIT]") {
                prowl::Priority::Emergency
            } else if alertname.starts_with("[high]") || alertname.starts_with("[HIGH]") {
                prowl::Priority::High
            } else {
                prowl::Priority::Normal
            }
        } else {
            prowl::Priority::VeryLow
        }
    }
}
