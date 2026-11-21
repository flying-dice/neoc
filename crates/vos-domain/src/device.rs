//! Paired devices: the browsers (and other clients) a person has authorised to use this host.

use serde::{Deserialize, Serialize};

/// Opaque identifier of one paired device (not a secret; the session token is).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

/// One paired device as the host knows it. Times are unix milliseconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: DeviceId,
    /// What the person (or the user agent) called it: "Safari on iPad".
    pub name: String,
    pub created_at: u64,
    pub last_seen_at: u64,
}

/// Best-effort device label from a `User-Agent` header, used when the client gives no name.
#[must_use]
pub fn device_name_from_user_agent(user_agent: &str) -> String {
    let ua = user_agent;
    let browser = if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("Firefox/") {
        "Firefox"
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") {
        "Chrome"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        ""
    };
    let platform = if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Mac OS X") || ua.contains("Macintosh") {
        "Mac"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        ""
    };
    match (browser, platform) {
        ("", "") => "Unknown device".to_owned(),
        (b, "") => b.to_owned(),
        ("", p) => p.to_owned(),
        (b, p) => format!("{b} on {p}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_device_from_its_user_agent() {
        let ipad =
            "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Version/18.0 Safari/605.1.15";
        assert_eq!(device_name_from_user_agent(ipad), "Safari on iPad");
        let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/130.0 Safari/537.36";
        assert_eq!(device_name_from_user_agent(chrome), "Chrome on Mac");
        assert_eq!(device_name_from_user_agent("curl/8.0"), "Unknown device");
    }

    #[test]
    fn serialises_with_camel_case_keys() {
        let d = Device { id: DeviceId("d1".into()), name: "n".into(), created_at: 1, last_seen_at: 2 };
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({"id":"d1","name":"n","createdAt":1,"lastSeenAt":2})
        );
    }
}
