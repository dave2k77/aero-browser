use aero_agent::data_flow::{
    Classification, DispatchError, DispatchHandle, GatewayHost, Grant, MAX_PAYLOAD_BYTES, Payload,
    PayloadKind, Purpose, Recipient, RecipientKind, Transport, TransportError,
};
use aero_core::{
    checkpoint::RunContext,
    ids::{RunId, TaskId},
    task::ProcessingMode,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

#[derive(Default)]
struct Recorder(Mutex<Vec<Vec<u8>>>);
impl Transport for Recorder {
    fn send(&self, bytes: &[u8]) -> Result<(), TransportError> {
        self.0.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
}
fn context(revision: u64) -> RunContext {
    RunContext {
        task_id: TaskId::from("task"),
        run_id: RunId::from("run"),
        contract_revision: revision,
    }
}
fn setup(
    mode: ProcessingMode,
    classification: Option<Classification>,
) -> (GatewayHost, Arc<Recorder>, Grant) {
    let host = GatewayHost::new(context(1), mode).unwrap();
    let recorder = Arc::new(Recorder::default());
    host.install_recipient(
        Recipient {
            id: "recipient".into(),
            kind: RecipientKind::Remote,
        },
        recorder.clone(),
    )
    .unwrap();
    host.install_payload(Payload {
        id: "payload".into(),
        source: "source".into(),
        classification,
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: b"actual encoded bytes".to_vec(),
    })
    .unwrap();
    let grant = Grant {
        id: "grant".into(),
        context: context(1),
        source: "source".into(),
        classification: classification.unwrap_or(Classification::Private),
        payload_id: "payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: b"actual encoded bytes".len(),
        expires_at: SystemTime::now() + Duration::from_secs(60),
    };
    (host, recorder, grant)
}
fn count(r: &Recorder) -> usize {
    r.0.lock().unwrap().len()
}

struct ReentrantDrop(DispatchHandle);
impl Transport for ReentrantDrop {
    fn send(&self, _: &[u8]) -> Result<(), TransportError> {
        Ok(())
    }
}
impl Drop for ReentrantDrop {
    fn drop(&mut self) {
        let _ = self.0.dispatch("payload", "grant");
    }
}
#[test]
fn retiring_transport_with_reentrant_destructor_does_not_deadlock() {
    if std::env::var_os("AERO_FLOW_DROP_CHILD").is_some() {
        let host = GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap();
        let transport = Arc::new(ReentrantDrop(host.dispatch_handle()));
        host.install_recipient(
            Recipient {
                id: "recipient".into(),
                kind: RecipientKind::Remote,
            },
            transport,
        )
        .unwrap();
        host.retire_run().unwrap();
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "retiring_transport_with_reentrant_destructor_does_not_deadlock",
        ])
        .env("AERO_FLOW_DROP_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("transport destructor deadlocked");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct CyclicPanicTransport(Arc<GatewayHost>);
impl Transport for CyclicPanicTransport {
    fn send(&self, _: &[u8]) -> Result<(), TransportError> {
        let _keep_host_alive = &self.0;
        panic!("synthetic transport failure")
    }
}

#[test]
fn poisoned_retirement_releases_host_transport_cycle() {
    let host = Arc::new(GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap());
    let weak_host = Arc::downgrade(&host);
    let transport = Arc::new(CyclicPanicTransport(Arc::clone(&host)));
    let weak_transport = Arc::downgrade(&transport);
    host.install_recipient(
        Recipient {
            id: "recipient".into(),
            kind: RecipientKind::Remote,
        },
        transport,
    )
    .unwrap();
    host.install_payload(Payload {
        id: "payload".into(),
        source: "source".into(),
        classification: None,
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: vec![1],
    })
    .unwrap();
    host.install_grant(Grant {
        id: "grant".into(),
        context: context(1),
        source: "source".into(),
        classification: Classification::Private,
        payload_id: "payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: 1,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    let handle = host.dispatch_handle();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || handle.dispatch("payload", "grant")
        ))
        .is_err()
    );
    assert_eq!(host.retire_run(), Err(DispatchError::Unavailable));
    assert!(weak_transport.upgrade().is_none());
    drop(host);
    assert!(weak_host.upgrade().is_none());
    assert_eq!(
        handle.dispatch("payload", "grant"),
        Err(DispatchError::Unavailable)
    );
}

struct PanicTransport;
impl Transport for PanicTransport {
    fn send(&self, _: &[u8]) -> Result<(), TransportError> {
        panic!("synthetic transport failure")
    }
}

#[test]
fn poisoned_host_drop_releases_transport_despite_live_handle() {
    let host = GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap();
    let transport = Arc::new(PanicTransport);
    let weak = Arc::downgrade(&transport);
    host.install_recipient(
        Recipient {
            id: "recipient".into(),
            kind: RecipientKind::Remote,
        },
        transport,
    )
    .unwrap();
    host.install_payload(Payload {
        id: "payload".into(),
        source: "source".into(),
        classification: None,
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: vec![1],
    })
    .unwrap();
    host.install_grant(Grant {
        id: "grant".into(),
        context: context(1),
        source: "source".into(),
        classification: Classification::Private,
        payload_id: "payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: 1,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    let handle = host.dispatch_handle();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || handle.dispatch("payload", "grant")
        ))
        .is_err()
    );
    drop(host);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        handle.dispatch("payload", "grant"),
        Err(DispatchError::Unavailable)
    );
}

#[test]
fn inference_handle_cannot_use_an_export_grant() {
    let (host, recorder, mut grant) = setup(ProcessingMode::ApprovedCloud, None);
    grant.purpose = Purpose::Export;
    host.install_grant(grant).unwrap();
    let inference = host.dispatch_handle();
    assert_eq!(
        inference.dispatch_for("payload", "grant", Purpose::Export),
        Err(DispatchError::Denied)
    );
    assert_eq!(count(&recorder), 0);
    host.dispatch_handle_for(Purpose::Export)
        .dispatch_for("payload", "grant", Purpose::Export)
        .unwrap();
    assert_eq!(count(&recorder), 1);
}

#[test]
fn retired_run_invalidates_all_retained_handles() {
    let (host, recorder, grant) = setup(ProcessingMode::ApprovedCloud, None);
    host.install_grant(grant).unwrap();
    let client = host.dispatch_handle();
    host.retire_run().unwrap();
    assert_eq!(
        host.advance_revision(context(2), ProcessingMode::ApprovedCloud),
        Err(DispatchError::Denied)
    );
    assert_eq!(
        host.install_payload(Payload {
            id: "new".into(),
            source: "source".into(),
            classification: None,
            secret: false,
            kind: PayloadKind::Text,
            encoded_bytes: vec![]
        }),
        Err(DispatchError::Denied)
    );
    assert_eq!(
        client.dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
    assert_eq!(count(&recorder), 0);
}

#[test]
fn recipient_and_lifetime_grant_counts_are_bounded() {
    let (host, recorder, mut grant) = setup(ProcessingMode::ApprovedCloud, None);
    for n in 0..64 {
        grant.id = format!("grant-{n}");
        host.install_grant(grant.clone()).unwrap();
        host.revoke_grant(&grant.id).unwrap();
    }
    grant.id = "one-too-many".into();
    assert_eq!(host.install_grant(grant), Err(DispatchError::Denied));
    for n in 0..63 {
        host.install_recipient(
            Recipient {
                id: format!("other-{n}"),
                kind: RecipientKind::Remote,
            },
            recorder.clone(),
        )
        .unwrap();
    }
    assert_eq!(
        host.install_recipient(
            Recipient {
                id: "one-too-many".into(),
                kind: RecipientKind::Remote
            },
            recorder.clone()
        ),
        Err(DispatchError::Denied)
    );
    assert_eq!(count(&recorder), 0);
}

#[test]
fn payload_count_and_total_encoded_bytes_are_bounded() {
    let host = GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap();
    for n in 0..8 {
        host.install_payload(Payload {
            id: format!("payload-{n}"),
            source: "source".into(),
            classification: None,
            secret: false,
            kind: PayloadKind::Image,
            encoded_bytes: vec![0; MAX_PAYLOAD_BYTES],
        })
        .unwrap();
    }
    assert_eq!(
        host.install_payload(Payload {
            id: "excess".into(),
            source: "source".into(),
            classification: None,
            secret: false,
            kind: PayloadKind::Text,
            encoded_bytes: vec![0],
        }),
        Err(DispatchError::Denied)
    );
    let count_host = GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap();
    for n in 0..64 {
        count_host
            .install_payload(Payload {
                id: format!("p-{n}"),
                source: "source".into(),
                classification: None,
                secret: false,
                kind: PayloadKind::Text,
                encoded_bytes: vec![],
            })
            .unwrap();
    }
    assert_eq!(
        count_host.install_payload(Payload {
            id: "extra".into(),
            source: "source".into(),
            classification: None,
            secret: false,
            kind: PayloadKind::Text,
            encoded_bytes: vec![],
        }),
        Err(DispatchError::Denied)
    );
}

#[test]
fn retained_handle_cannot_dispatch_after_host_drops() {
    let (host, recorder, grant) = setup(ProcessingMode::ApprovedCloud, None);
    host.install_grant(grant).unwrap();
    let client = host.dispatch_handle();
    drop(host);
    assert_eq!(
        client.dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
    assert_eq!(count(&recorder), 0);
}

#[test]
fn telemetry_cannot_send_task_content_even_with_a_grant() {
    let (host, recorder, mut grant) = setup(ProcessingMode::ApprovedCloud, None);
    grant.purpose = Purpose::Telemetry;
    host.install_grant(grant).unwrap();
    assert_eq!(client_telemetry(&host), Err(DispatchError::Denied));
    assert_eq!(count(&recorder), 0);
}

fn client_telemetry(host: &GatewayHost) -> Result<(), DispatchError> {
    host.dispatch_handle_for(Purpose::Telemetry).dispatch_for(
        "payload",
        "grant",
        Purpose::Telemetry,
    )
}

#[test]
fn transfer_requires_exact_scoped_grant() {
    for classification in [
        Some(Classification::Public),
        Some(Classification::Private),
        None,
    ] {
        let (host, recorder, grant) = setup(ProcessingMode::ApprovedCloud, classification);
        let client = host.dispatch_handle();
        assert!(client.dispatch("payload", "grant").is_err());
        assert_eq!(count(&recorder), 0);
        host.install_grant(grant).unwrap();
        client.dispatch("payload", "grant").unwrap();
        assert_eq!(
            *recorder.0.lock().unwrap(),
            vec![b"actual encoded bytes".to_vec()]
        );
    }
}

#[test]
fn scoped_mismatches_never_dispatch() {
    for case in 0..11 {
        let (host, recorder, mut grant) = setup(ProcessingMode::ApprovedCloud, None);
        match case {
            0 => grant.context.task_id = TaskId::from("other"),
            1 => grant.context.run_id = RunId::from("other"),
            2 => grant.context.contract_revision = 2,
            3 => grant.source = "other".into(),
            4 => grant.classification = Classification::Public,
            5 => grant.payload_id = "other".into(),
            6 => grant.recipient_id = "other".into(),
            7 => grant.purpose = Purpose::Export,
            8 => grant.payload_kind = PayloadKind::Image,
            9 => grant.encoded_size += 1,
            10 => grant.expires_at = SystemTime::now() - Duration::from_secs(1),
            _ => unreachable!(),
        }
        let result = host.install_grant(grant);
        if matches!(case, 7 | 10) {
            assert!(result.is_ok(), "case {case} must reach dispatch");
            assert_eq!(
                host.dispatch_handle()
                    .dispatch_for("payload", "grant", Purpose::Inference),
                Err(DispatchError::Denied)
            );
        } else {
            assert_eq!(result, Err(DispatchError::Denied), "case {case}");
        }
        assert_eq!(count(&recorder), 0, "case {case}");
    }
}

#[test]
fn expiry_revocation_and_revision_invalidate_handles() {
    let (host, recorder, mut grant) = setup(ProcessingMode::ApprovedCloud, None);
    grant.expires_at = SystemTime::now();
    host.install_grant(grant).unwrap();
    assert_eq!(
        host.dispatch_handle().dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
    assert_eq!(count(&recorder), 0);
    let (host, recorder, grant) = setup(ProcessingMode::ApprovedCloud, None);
    host.install_grant(grant.clone()).unwrap();
    let client = host.dispatch_handle();
    host.revoke_grant("grant").unwrap();
    assert!(client.dispatch("payload", "grant").is_err());
    assert!(host.install_grant(grant).is_err());
    host.advance_revision(context(2), ProcessingMode::ApprovedCloud)
        .unwrap();
    assert!(client.dispatch("payload", "grant").is_err());
    assert_eq!(count(&recorder), 0);
}

#[test]
fn local_processing_never_falls_back_remote() {
    for mode in [ProcessingMode::LocalOnly, ProcessingMode::Sensitive] {
        let (host, recorder, grant) = setup(mode, None);
        host.install_grant(grant).unwrap();
        assert!(host.dispatch_handle().dispatch("payload", "grant").is_err());
        assert_eq!(count(&recorder), 0);
    }
}

#[test]
fn bounds_and_sensitive_or_secret_data_fail_closed() {
    for (classification, secret, size) in [
        (Some(Classification::Sensitive), false, 1),
        (None, true, 1),
        (None, false, MAX_PAYLOAD_BYTES + 1),
    ] {
        let host = GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap();
        let recorder = Arc::new(Recorder::default());
        host.install_recipient(
            Recipient {
                id: "recipient".into(),
                kind: RecipientKind::Remote,
            },
            recorder.clone(),
        )
        .unwrap();
        let installed = host.install_payload(Payload {
            id: "payload".into(),
            source: "source".into(),
            classification,
            secret,
            kind: PayloadKind::Image,
            encoded_bytes: vec![1; size],
        });
        if size > MAX_PAYLOAD_BYTES {
            assert_eq!(installed, Err(DispatchError::Denied));
        } else {
            installed.unwrap();
            let grant = Grant {
                id: "grant".into(),
                context: context(1),
                source: "source".into(),
                classification: classification.unwrap_or(Classification::Private),
                payload_id: "payload".into(),
                recipient_id: "recipient".into(),
                purpose: Purpose::Inference,
                payload_kind: PayloadKind::Image,
                encoded_size: size,
                expires_at: SystemTime::now() + Duration::from_secs(60),
            };
            host.install_grant(grant).unwrap();
            assert_eq!(
                host.dispatch_handle().dispatch("payload", "grant"),
                Err(DispatchError::Denied)
            );
        }
        assert_eq!(count(&recorder), 0);
    }
}

#[test]
fn expiry_after_install_and_new_revision_with_new_grant() {
    let (host, recorder, mut grant) = setup(ProcessingMode::ApprovedCloud, None);
    grant.expires_at = SystemTime::now() + Duration::from_millis(30);
    host.install_grant(grant).unwrap();
    let old = host.dispatch_handle();
    std::thread::sleep(Duration::from_millis(70));
    assert!(old.dispatch("payload", "grant").is_err());
    host.advance_revision(context(2), ProcessingMode::ApprovedCloud)
        .unwrap();
    host.install_payload(Payload {
        id: "new-payload".into(),
        source: "source".into(),
        classification: None,
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: b"new".to_vec(),
    })
    .unwrap();
    host.install_grant(Grant {
        id: "new-grant".into(),
        context: context(2),
        source: "source".into(),
        classification: Classification::Private,
        payload_id: "new-payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: 3,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    assert!(old.dispatch("new-payload", "new-grant").is_err());
    host.dispatch_handle()
        .dispatch("new-payload", "new-grant")
        .unwrap();
    assert_eq!(*recorder.0.lock().unwrap(), vec![b"new".to_vec()]);
}

struct BlockingTransport {
    started: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl Transport for BlockingTransport {
    fn send(&self, _: &[u8]) -> Result<(), TransportError> {
        self.started.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(())
    }
}

#[test]
fn retirement_waits_for_inflight_send_then_denies_later_requests() {
    use std::sync::mpsc::channel;
    let (started_tx, started_rx) = channel();
    let (release_tx, release_rx) = channel();
    let host = Arc::new(GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap());
    host.install_recipient(
        Recipient {
            id: "recipient".into(),
            kind: RecipientKind::Remote,
        },
        Arc::new(BlockingTransport {
            started: started_tx,
            release: Mutex::new(release_rx),
        }),
    )
    .unwrap();
    host.install_payload(Payload {
        id: "payload".into(),
        source: "source".into(),
        classification: None,
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: b"one".to_vec(),
    })
    .unwrap();
    host.install_grant(Grant {
        id: "grant".into(),
        context: context(1),
        source: "source".into(),
        classification: Classification::Private,
        payload_id: "payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: 3,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    let client = host.dispatch_handle();
    let sender = client.clone();
    let dispatch = std::thread::spawn(move || sender.dispatch("payload", "grant"));
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (done_tx, done_rx) = channel();
    let retiring = host.clone();
    let worker = std::thread::spawn(move || {
        done_tx.send(retiring.retire_run()).unwrap();
    });
    assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    assert!(dispatch.join().unwrap().is_ok());
    assert!(
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_ok()
    );
    worker.join().unwrap();
    assert_eq!(
        client.dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
}

#[test]
fn revocation_waits_for_admitted_dispatch_and_then_denies() {
    use std::sync::mpsc::channel;
    let (started_tx, started_rx) = channel();
    let (release_tx, release_rx) = channel();
    let host = Arc::new(GatewayHost::new(context(1), ProcessingMode::ApprovedCloud).unwrap());
    host.install_recipient(
        Recipient {
            id: "recipient".into(),
            kind: RecipientKind::Remote,
        },
        Arc::new(BlockingTransport {
            started: started_tx,
            release: Mutex::new(release_rx),
        }),
    )
    .unwrap();
    host.install_payload(Payload {
        id: "payload".into(),
        source: "source".into(),
        classification: None,
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: b"one".to_vec(),
    })
    .unwrap();
    host.install_grant(Grant {
        id: "grant".into(),
        context: context(1),
        source: "source".into(),
        classification: Classification::Private,
        payload_id: "payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: 3,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    let client = host.dispatch_handle();
    let dispatch_client = client.clone();
    let dispatch = std::thread::spawn(move || dispatch_client.dispatch("payload", "grant"));
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (revoking_tx, revoking_rx) = channel();
    let (revoked_tx, revoked_rx) = channel();
    let revoker_host = host.clone();
    let revoker = std::thread::spawn(move || {
        revoking_tx.send(()).unwrap();
        let result = revoker_host.revoke_grant("grant");
        revoked_tx.send(result).unwrap();
    });
    revoking_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(revoked_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    assert!(dispatch.join().unwrap().is_ok());
    assert!(
        revoked_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_ok()
    );
    revoker.join().unwrap();
    assert_eq!(
        client.dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
}

#[test]
fn grants_bind_preinstalled_immutable_bytes_and_transport() {
    let (host, recorder, grant) = setup(ProcessingMode::ApprovedCloud, None);
    let other_recorder = Arc::new(Recorder::default());
    assert!(
        host.install_payload(Payload {
            id: "payload".into(),
            source: "source".into(),
            classification: None,
            secret: false,
            kind: PayloadKind::Text,
            encoded_bytes: b"replacement".to_vec()
        })
        .is_err()
    );
    assert!(
        host.install_recipient(
            Recipient {
                id: "recipient".into(),
                kind: RecipientKind::Remote
            },
            other_recorder.clone()
        )
        .is_err()
    );
    let mut missing = grant.clone();
    missing.id = "missing".into();
    missing.payload_id = "future".into();
    assert!(host.install_grant(missing).is_err());
    host.install_grant(grant).unwrap();
    host.dispatch_handle().dispatch("payload", "grant").unwrap();
    assert_eq!(
        *recorder.0.lock().unwrap(),
        vec![b"actual encoded bytes".to_vec()]
    );
    assert_eq!(count(&other_recorder), 0);
}

#[test]
fn local_recipient_and_exact_max_encoded_size() {
    let host = GatewayHost::new(context(1), ProcessingMode::LocalOnly).unwrap();
    let recorder = Arc::new(Recorder::default());
    host.install_recipient(
        Recipient {
            id: "local".into(),
            kind: RecipientKind::Local,
        },
        recorder.clone(),
    )
    .unwrap();
    host.install_payload(Payload {
        id: "image".into(),
        source: "source".into(),
        classification: None,
        secret: false,
        kind: PayloadKind::Image,
        encoded_bytes: vec![42; MAX_PAYLOAD_BYTES],
    })
    .unwrap();
    host.install_grant(Grant {
        id: "grant".into(),
        context: context(1),
        source: "source".into(),
        classification: Classification::Private,
        payload_id: "image".into(),
        recipient_id: "local".into(),
        purpose: Purpose::Ocr,
        payload_kind: PayloadKind::Image,
        encoded_size: MAX_PAYLOAD_BYTES,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    host.dispatch_handle_for(Purpose::Ocr)
        .dispatch_for("image", "grant", Purpose::Ocr)
        .unwrap();
    assert_eq!(
        recorder.0.lock().unwrap().as_slice(),
        &[vec![42; MAX_PAYLOAD_BYTES]]
    );
}

#[test]
fn optional_adapter_cannot_bypass_mode() {
    let (host, recorder, mut grant) =
        setup(ProcessingMode::LocalOnly, Some(Classification::Public));
    grant.purpose = Purpose::Tool;
    host.install_grant(grant).unwrap();
    assert_eq!(
        host.dispatch_handle_for(Purpose::Tool)
            .dispatch_for("payload", "grant", Purpose::Tool),
        Err(DispatchError::Denied)
    );
    assert_eq!(count(&recorder), 0);
}
