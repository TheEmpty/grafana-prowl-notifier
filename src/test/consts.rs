pub(crate) fn create_firing_alert() -> String {
    create_firing_alert_with_prefix("")
}

pub(crate) fn create_resolved_alert() -> String {
    create_resolved_alert_with_prefix("")
}

pub(crate) fn create_firing_alert_with_prefix(prefix: &str) -> String {
    format!("{{\"status\": \"firing\", \"generatorURL\": \"http://something/this\", \"fingerprint\": \"581dd91e73c77248\", \"labels\": {{ \"alertname\": \"{prefix}Alert Name\" }}, \"annotations\": {{ \"summary\": \"Annotation Summary\"}}}}")
}

pub(crate) fn create_resolved_alert_with_prefix(prefix: &str) -> String {
    format!("{{\"status\": \"resolved\", \"generatorURL\": \"http://something/this\", \"fingerprint\": \"581dd91e73c77248\", \"labels\": {{ \"alertname\": \"{prefix}Alert Name\" }}, \"annotations\": {{ \"summary\": \"Annotation Summary\"}}}}")
}
