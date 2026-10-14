// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Actual loopback TLS reads with a per-agent test root, never host trust changes.

use super::*;
use openssl::asn1::Asn1Time;
use openssl::bn::BigNum;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::ssl::{SslAcceptor, SslMethod};
use openssl::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use openssl::x509::{X509NameBuilder, X509};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use ureq::tls::{Certificate, RootCerts, TlsConfig};

fn tls_identity() -> (SslAcceptor, TlsConfig) {
    let root_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let leaf_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "BlueIce owned TLS test root")
        .unwrap();
    let root_name = name.build();
    let mut root = X509::builder().unwrap();
    root.set_version(2).unwrap();
    root.set_serial_number(
        BigNum::from_u32(1)
            .unwrap()
            .to_asn1_integer()
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    root.set_subject_name(&root_name).unwrap();
    root.set_issuer_name(&root_name).unwrap();
    root.set_pubkey(&root_key).unwrap();
    root.set_not_before(Asn1Time::days_from_now(0).unwrap().as_ref())
        .unwrap();
    root.set_not_after(Asn1Time::days_from_now(1).unwrap().as_ref())
        .unwrap();
    root.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
        .unwrap();
    root.append_extension(KeyUsage::new().critical().key_cert_sign().build().unwrap())
        .unwrap();
    root.sign(&root_key, MessageDigest::sha256()).unwrap();
    let root = root.build();
    let mut leaf = X509::builder().unwrap();
    leaf.set_version(2).unwrap();
    leaf.set_serial_number(
        BigNum::from_u32(2)
            .unwrap()
            .to_asn1_integer()
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    leaf.set_subject_name(&root_name).unwrap();
    leaf.set_issuer_name(root.subject_name()).unwrap();
    leaf.set_pubkey(&leaf_key).unwrap();
    leaf.set_not_before(Asn1Time::days_from_now(0).unwrap().as_ref())
        .unwrap();
    leaf.set_not_after(Asn1Time::days_from_now(1).unwrap().as_ref())
        .unwrap();
    leaf.append_extension(BasicConstraints::new().critical().build().unwrap())
        .unwrap();
    leaf.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .build()
            .unwrap(),
    )
    .unwrap();
    leaf.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
        .unwrap();
    let san = SubjectAlternativeName::new()
        .ip("127.0.0.1")
        .build(&leaf.x509v3_context(Some(&root), None))
        .unwrap();
    leaf.append_extension(san).unwrap();
    leaf.sign(&root_key, MessageDigest::sha256()).unwrap();
    let leaf = leaf.build();
    let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    acceptor.set_private_key(&leaf_key).unwrap();
    acceptor.set_certificate(&leaf).unwrap();
    acceptor.check_private_key().unwrap();
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::new_with_certs(&[Certificate::from_pem(
            &root.to_pem().unwrap(),
        )
        .unwrap()]))
        .build();
    (acceptor.build(), tls)
}

fn exercise_tls(cancel: bool) {
    let (acceptor, tls) = tls_identity();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("https://{}/original", listener.local_addr().unwrap());
    let origin = thread::spawn(move || -> io::Result<bool> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let tcp = loop {
            match listener.accept() {
                Ok((tcp, _)) => break tcp,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "no TLS test connection",
                        ));
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                Err(error) => return Err(error),
            }
        };
        tcp.set_nonblocking(false)?;
        tcp.set_read_timeout(Some(Duration::from_secs(3)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(3)))?;
        // Drop a failed handshake's owned socket rather than retain it in Error.
        let mut stream = acceptor
            .accept(tcp)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            if stream.read(&mut byte)? == 0 || request.len() >= 16_384 {
                return Err(io::Error::other("missing or oversized TLS request"));
            }
            request.push(byte[0]);
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n")?;
        stream.flush()?;
        if cancel {
            stream
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(2)))?;
            Ok(match stream.read(&mut [0]) {
                Ok(0) => true,
                Err(error) => !matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ),
                _ => false,
            })
        } else {
            // Several polling deadlines elapse while the TLS body is held.
            thread::sleep(Duration::from_millis(350));
            stream.write_all(&[0xff, 0, 0x80, 0xfe, 13, 10, 42])?;
            stream.flush()?;
            Ok(true)
        }
    });
    let cancellation = ResponseCancellation::default();
    let agent = agent_with_config(
        ureq::config::Config::builder()
            .tls_config(tls)
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_recv_response(Some(Duration::from_secs(3)))
            .build(),
        cancellation.clone(),
    );
    let mut response = match agent.get(&url).call() {
        Ok(response) => response,
        Err(error) => {
            let origin = origin.join();
            panic!("TLS response failed: {error}; origin: {origin:?}");
        }
    };
    let read = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = response.body_mut().as_reader().read_to_end(&mut bytes);
        (result, bytes)
    });
    if cancel {
        thread::sleep(Duration::from_millis(250));
        cancellation.cancel();
    }
    let (result, bytes) = read.join().unwrap();
    let closed = origin.join().unwrap().unwrap();
    assert!(
        closed,
        "TLS origin must observe closure within its two-second deadline"
    );
    if cancel {
        assert!(result.is_err());
        assert!(bytes.is_empty());
    } else {
        assert_eq!(result.unwrap(), 7);
        assert_eq!(bytes, [0xff, 0, 0x80, 0xfe, 13, 10, 42]);
    }
}

#[test]
fn delayed_tls_body_survives_multiple_transport_polls() {
    exercise_tls(false);
}

#[test]
fn stalled_tls_body_cancellation_closes_the_original_connection() {
    exercise_tls(true);
}
