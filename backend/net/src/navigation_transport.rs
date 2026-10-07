// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cancellation on individual transport reads, without a total-body time limit.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport,
};

#[derive(Clone, Debug, Default)]
pub struct ResponseCancellation(Arc<AtomicBool>);
impl ResponseCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub(crate) fn agent(cancellation: ResponseCancellation) -> ureq::Agent {
    let config = ureq::config::Config::builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build();
    agent_with_config(config, cancellation)
}

fn agent_with_config(
    config: ureq::config::Config,
    cancellation: ResponseCancellation,
) -> ureq::Agent {
    ureq::Agent::with_parts(
        config,
        DefaultConnector::default().chain(CancellableConnector(cancellation)),
        DefaultResolver::default(),
    )
}

#[cfg(all(test, unix))]
#[path = "../tests/unit/navigation_transport.rs"]
mod tests;

#[derive(Debug)]
struct CancellableConnector(ResponseCancellation);
impl Connector<Box<dyn Transport>> for CancellableConnector {
    type Out = Box<dyn Transport>;
    fn connect(
        &self,
        _details: &ConnectionDetails,
        chained: Option<Box<dyn Transport>>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| {
            Box::new(CancellableTransport {
                inner,
                cancellation: self.0.clone(),
            }) as Self::Out
        }))
    }
}

#[derive(Debug)]
struct CancellableTransport {
    inner: Box<dyn Transport>,
    cancellation: ResponseCancellation,
}
impl Transport for CancellableTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.inner.transmit_output(amount, timeout)
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        // Preserve a shorter caller deadline. Otherwise each wait for another
        // byte has the existing downloader's 30-second idle budget; a healthy
        // long response may continue indefinitely while making progress.
        let budget = timeout
            .not_zero()
            .map(|duration| *duration)
            .unwrap_or(Duration::from_secs(30))
            .min(Duration::from_secs(30));
        let deadline = Instant::now() + budget;
        loop {
            if self.cancellation.cancelled() {
                return Err(ureq::Error::Io(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "navigation response cancelled",
                )));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ureq::Error::Timeout(timeout.reason));
            }
            let poll = NextTimeout {
                after: remaining.min(Duration::from_millis(100)).into(),
                reason: timeout.reason,
            };
            match self.inner.await_input(poll) {
                // Retry inside the transport, retaining HTTP/TLS parser state.
                Err(ureq::Error::Timeout(_)) => continue,
                other => return other,
            }
        }
    }
    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }
    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}
