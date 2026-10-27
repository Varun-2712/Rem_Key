use std::net::SocketAddr;

use pir_pairing::pairing::PairingInfo;
use pir_transport::quic::CertFingerprint;

use crate::errors::RelayError;

/// A desktop agent the phone can connect to, as learned from its pairing code. The app
/// keeps this after pairing and passes it back to `RelayClient::connect`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DesktopTarget {
    pub name: String,
    /// `host:port` addresses, tried in order.
    pub endpoints: Vec<String>,
    /// SHA-256 of the agent's certificate, hex.
    pub fingerprint: String,
}

/// What the desktop's pairing QR code carries: where it is, who it is, and the one-time
/// secret that admits a new phone.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairingCode {
    pub desktop: DesktopTarget,
    pub secret: String,
}

/// Reads the text of a pairing QR code (the JSON the agent prints with `--pair`).
#[uniffi::export]
pub fn parse_pairing_code(text: String) -> Result<PairingCode, RelayError> {
    let info = PairingInfo::from_json(text.trim())
        .map_err(|e| RelayError::InvalidPairingCode { message: e.to_string() })?;
    let code = PairingCode {
        desktop: DesktopTarget { name: info.name, endpoints: info.endpoints, fingerprint: info.cert_fingerprint },
        secret: info.secret,
    };
    // Reject what connect would reject, so the app can say so before trying.
    parse_endpoints(&code.desktop.endpoints)?;
    parse_fingerprint(&code.desktop.fingerprint)?;
    Ok(code)
}

pub(crate) fn parse_endpoints(endpoints: &[String]) -> Result<Vec<SocketAddr>, RelayError> {
    if endpoints.is_empty() {
        return Err(RelayError::InvalidAddress { message: "no desktop address".into() });
    }
    endpoints.iter().map(|e| parse_endpoint(e)).collect()
}

fn parse_endpoint(endpoint: &str) -> Result<SocketAddr, RelayError> {
    let endpoint = endpoint.trim();
    endpoint
        .parse()
        .or_else(|_| {
            // A bare IPv6 address with a port, as people tend to type it.
            let (host, port) = endpoint.rsplit_once(':').ok_or(())?;
            format!("[{host}]:{port}").parse().map_err(|_| ())
        })
        .map_err(|_| RelayError::InvalidAddress { message: endpoint.to_string() })
}

pub(crate) fn parse_fingerprint(hex: &str) -> Result<CertFingerprint, RelayError> {
    let hex: String = hex.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if hex.len() != 64 {
        return Err(RelayError::InvalidFingerprint { message: "expected 64 hex characters".into() });
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| RelayError::InvalidFingerprint { message: "not hexadecimal".into() })?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qr(endpoints: &str) -> String {
        format!(
            r#"{{"version":2,"name":"Desk","endpoints":[{endpoints}],"cert_fingerprint":"{}","secret":"s3cret"}}"#,
            "ab".repeat(32)
        )
    }

    #[test]
    fn reads_the_agent_qr_payload() {
        let code = parse_pairing_code(qr(r#""192.168.1.5:8080""#)).unwrap();
        assert_eq!(code.desktop.name, "Desk");
        assert_eq!(code.desktop.endpoints, vec!["192.168.1.5:8080"]);
        assert_eq!(code.secret, "s3cret");
    }

    #[test]
    fn rejects_bad_pairing_codes() {
        assert!(matches!(parse_pairing_code("hello".into()), Err(RelayError::InvalidPairingCode { .. })));
        assert!(matches!(parse_pairing_code(qr("")), Err(RelayError::InvalidAddress { .. })));
        assert!(matches!(parse_pairing_code(qr(r#""nowhere""#)), Err(RelayError::InvalidAddress { .. })));
        let old = qr(r#""10.0.0.1:1""#).replace("\"version\":2", "\"version\":1");
        assert!(matches!(parse_pairing_code(old), Err(RelayError::InvalidPairingCode { .. })));
    }

    #[test]
    fn endpoints_accept_ipv4_and_ipv6() {
        let parsed = parse_endpoints(&["10.0.0.2:80".into(), "[::1]:9".into(), "fe80::1:9".into()]).unwrap();
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn fingerprint_accepts_colons_and_rejects_bad_input() {
        let hex = "ab".repeat(32);
        assert_eq!(parse_fingerprint(&hex).unwrap(), [0xab; 32]);
        let colons = vec!["AB"; 32].join(":");
        assert_eq!(parse_fingerprint(&colons).unwrap(), [0xab; 32]);
        assert!(parse_fingerprint("abcd").is_err());
        assert!(parse_fingerprint(&"zz".repeat(32)).is_err());
    }
}
