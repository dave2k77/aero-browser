//! Full-product browser scenarios (implementation plan §9). They compile only
//! with `--features browser-tests` and then require the pinned browser: a
//! missing prerequisite fails the test, it never skips.

#[cfg(feature = "browser-tests")]
mod support;

/// C07: the harness itself — a local fixture server and a sandboxed browser
/// are both reachable, and both are reaped on the normal and the failure path.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn fixture_harness_smoke() {
    use serde_json::json;
    use std::time::Duration;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");

    // The server answers the host directly.
    let response = support::http_get(fixture.server.address(), "/basic.html")
        .await
        .expect("fixture server reachable");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("<title>AeroBrowser basic fixture</title>"));
    // `..` is refused even when it would lead back to a real fixture.
    for path in [
        "/../fixtures/basic.html",
        "/../../Cargo.toml",
        "/BASIC.html",
    ] {
        let refused = support::http_get(fixture.server.address(), path)
            .await
            .unwrap();
        assert!(refused.starts_with("HTTP/1.1 404"), "{path}: {refused}");
    }

    // The browser answers over CDP and loads the fixture from the server.
    let version = fixture
        .cdp
        .call("Browser.getVersion", json!({}), None, support::CALL)
        .await
        .unwrap();
    assert!(version["product"].as_str().unwrap().contains("Chrome/"));
    let mut events = fixture.cdp.subscribe();
    fixture
        .cdp
        .call(
            "Target.setDiscoverTargets",
            json!({ "discover": true }),
            None,
            support::CALL,
        )
        .await
        .unwrap();
    let url = fixture.server.url("basic.html");
    let created = fixture
        .cdp
        .call(
            "Target.createTarget",
            json!({ "url": url }),
            None,
            support::CALL,
        )
        .await
        .unwrap();
    let target = created["targetId"].clone();
    // Event path: the new page target reports the fixture URL.
    support::wait_until(&mut events, Duration::from_secs(10), |event| {
        let info = &event.params["targetInfo"];
        (event.method == "Target.targetInfoChanged"
            && info["targetId"] == target
            && info["url"] == url.as_str())
        .then_some(())
    })
    .await
    .expect("the page target navigated to the fixture");
    // Poll path: the loaded document's title becomes the target title.
    let title = support::eventually(Duration::from_secs(10), || async {
        let targets = fixture
            .cdp
            .call("Target.getTargets", json!({}), None, support::CALL)
            .await
            .ok()?;
        targets["targetInfos"]
            .as_array()?
            .iter()
            .find(|info| info["targetId"] == target)
            .and_then(|info| info["title"].as_str())
            .filter(|title| *title == "AeroBrowser basic fixture")
            .map(str::to_owned)
    })
    .await;
    assert_eq!(
        title.as_deref(),
        Some("AeroBrowser basic fixture"),
        "requests: {:?}",
        fixture.server.requests()
    );
    assert!(
        fixture
            .server
            .requests()
            .contains(&"GET /basic.html".to_owned())
    );

    fixture
        .shutdown()
        .await
        .expect("server and browser teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());

    // Failure path: a scenario that panics still reaps browser and server.
    let (started, address) = tokio::sync::oneshot::channel();
    let crashed = tokio::spawn(async move {
        let fixture = support::Fixture::start()
            .await
            .expect("second fixture starts");
        assert!(!support::sandbox_processes().is_empty());
        started.send(fixture.server.address()).unwrap();
        panic!("simulated scenario failure");
    })
    .await;
    assert!(crashed.unwrap_err().is_panic());
    let address = address.await.unwrap();
    assert_eq!(
        support::sandbox_processes(),
        Vec::<u32>::new(),
        "browser reaped during unwinding"
    );
    assert!(
        support::refuses_connections(address, Duration::from_secs(2)).await,
        "fixture server stopped during unwinding"
    );
}
