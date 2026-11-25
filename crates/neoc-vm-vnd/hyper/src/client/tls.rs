//! The TLS client settings of `vnd:hyper`: rustls over the `ring` provider, the certificate checked by the platform
//! verifier (the operating system's trust store), or by exactly the certificates of `ca_pem` when the request gives
//! them (read and checked with the options, `options::ca_pem`).

use std::sync::Arc;

use tokio_rustls::{
    TlsConnector,
    rustls::{self, ClientConfig, RootCertStore},
};

use super::error::HyperError;

/// The connector for a request, built when the first `https` URL needs it and kept for the hops after it (`ca_pem`
/// applies to every hop).
pub(super) struct Tls {
    ca_pem: Option<RootCertStore>,
    built: Option<TlsConnector>,
}

impl Tls {
    pub(super) fn new(ca_pem: Option<RootCertStore>) -> Self {
        Self { ca_pem, built: None }
    }

    pub(super) fn connector(&mut self) -> Result<TlsConnector, HyperError> {
        if let Some(built) = &self.built {
            return Ok(built.clone());
        }
        let built = build(self.ca_pem.clone())?;
        self.built = Some(built.clone());
        Ok(built)
    }
}

fn build(ca_pem: Option<RootCertStore>) -> Result<TlsConnector, HyperError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| HyperError::tls(e.to_string()))?;
    let config = match ca_pem {
        None => {
            use rustls_platform_verifier::BuilderVerifierExt;
            builder
                .with_platform_verifier()
                .map_err(|e| {
                    HyperError::tls(format!("the platform certificate verifier is unavailable: {e}"))
                })?
                .with_no_client_auth()
        }
        Some(roots) => builder.with_root_certificates(roots).with_no_client_auth(),
    };
    Ok(TlsConnector::from(Arc::new(config)))
}
