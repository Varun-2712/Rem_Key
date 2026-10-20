use crate::events::ProtocolVersion;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VersionError {
    #[error("Unsupported protocol version: {0}")]
    Unsupported(ProtocolVersion),
    #[error("Version negotiation failed: client {client}, server {server}")]
    NegotiationFailed { client: ProtocolVersion, server: ProtocolVersion },
}

pub fn negotiate_version(client_version: ProtocolVersion, server_version: ProtocolVersion) -> Result<ProtocolVersion, VersionError> {
    if client_version < ProtocolVersion::MIN_COMPATIBLE || server_version < ProtocolVersion::MIN_COMPATIBLE {
        return Err(VersionError::Unsupported(client_version.min(server_version)));
    }
    
    // A major version bump means an incompatible wire format.
    if client_version.major != server_version.major {
        return Err(VersionError::NegotiationFailed { client: client_version, server: server_version });
    }

    // Within a major version, both sides speak the lower minor version.
    Ok(client_version.min(server_version))
}

pub fn is_compatible(client_version: ProtocolVersion, server_version: ProtocolVersion) -> bool {
    negotiate_version(client_version, server_version).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ProtocolVersion;

    #[test]
    fn test_version_negotiation() {
        let v1_0_0 = ProtocolVersion { major: 1, minor: 0, patch: 0 };
        let v1_1_0 = ProtocolVersion { major: 1, minor: 1, patch: 0 };
        let v2_0_0 = ProtocolVersion { major: 2, minor: 0, patch: 0 };
        
        assert_eq!(negotiate_version(v1_0_0, v1_0_0).unwrap(), v1_0_0);
        assert_eq!(negotiate_version(v1_0_0, v1_1_0).unwrap(), v1_0_0);
        assert_eq!(negotiate_version(v1_1_0, v1_0_0).unwrap(), v1_0_0);
        assert!(negotiate_version(v1_1_0, v2_0_0).is_err());
    }

    #[test]
    fn test_incompatible_versions() {
        let v0_9_0 = ProtocolVersion { major: 0, minor: 9, patch: 0 };
        let v1_0_0 = ProtocolVersion { major: 1, minor: 0, patch: 0 };
        
        assert!(negotiate_version(v0_9_0, v1_0_0).is_err());
    }
}