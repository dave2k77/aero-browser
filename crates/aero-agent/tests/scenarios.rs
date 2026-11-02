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
        .cdp()
        .call("Browser.getVersion", json!({}), None, support::CALL)
        .await
        .unwrap();
    assert!(version["product"].as_str().unwrap().contains("Chrome/"));
    let mut events = fixture.cdp().subscribe();
    fixture
        .cdp()
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
        .cdp()
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
            .cdp()
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

/// C05: one attachment owner, tabs exclude non-page targets, page methods go
/// to the page's own session, and navigation really changes title and URL.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn page_target_navigation() {
    use aero_cdp::CdpError;
    use aero_cdp::page::Page;
    use aero_cdp::runtime::RuntimeError;
    use aero_cdp::target_registry::{AttachError, TargetKind};
    use serde_json::{Value, json};
    use std::time::Duration;

    async fn document(page: &Page) -> Option<(String, String)> {
        let reply = page
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": "document.readyState === 'complete' ? [document.title, location.href] : null",
                    "returnByValue": true
                }),
            )
            .await
            .ok()?;
        match &reply["result"]["value"] {
            Value::Array(pair) => {
                Some((pair[0].as_str()?.to_owned(), pair[1].as_str()?.to_owned()))
            }
            _ => None,
        }
    }

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();

    // Existing targets were enumerated: the startup tab is known without
    // creating anything, and only real pages count as tabs.
    let tab = runtime
        .wait_for_tab(Duration::from_secs(10))
        .await
        .expect("the startup tab is discovered");
    assert_eq!(tab.kind, TargetKind::Page);
    for target in runtime.targets() {
        assert_eq!(
            runtime
                .tabs()
                .iter()
                .any(|t| t.target_id == target.target_id),
            target.kind == TargetKind::Page && !target.url.starts_with("devtools://"),
            "{target:?}"
        );
    }

    // One owner: an explicit flattened session, and no second one.
    let page = runtime
        .attach(&tab.target_id)
        .await
        .expect("attach the tab");
    assert_ne!(page.session().as_str(), tab.target_id.as_str());
    assert_eq!(
        runtime.attach(&tab.target_id).await.err(),
        Some(RuntimeError::Attach(AttachError::AlreadyAttached))
    );
    // Chromium always exposes non-tab targets (browser UI, extension service
    // workers); they are discovered but cannot be attached as tabs.
    let other = support::eventually(Duration::from_secs(5), || async {
        runtime.targets().into_iter().find(|t| !t.is_tab())
    })
    .await
    .expect("a non-tab target is discovered");
    assert!(
        !runtime
            .tabs()
            .iter()
            .any(|t| t.target_id == other.target_id)
    );
    assert_eq!(
        runtime.attach(&other.target_id).await.err(),
        Some(RuntimeError::Attach(AttachError::NotATab))
    );

    // Page methods belong to the page session; the browser endpoint is not a page.
    assert!(matches!(
        runtime
            .connection()
            .call(
                "Runtime.evaluate",
                json!({ "expression": "1" }),
                None,
                support::CALL
            )
            .await,
        Err(CdpError::Protocol { .. })
    ));

    // Navigation changes the actual document, observed through the session.
    for (name, title) in [
        ("basic.html", "AeroBrowser basic fixture"),
        ("second.html", "AeroBrowser second fixture"),
    ] {
        let url = fixture.server.url(name);
        page.navigate(&url).await.expect("navigation starts");
        let seen = support::eventually(Duration::from_secs(10), || async {
            document(&page)
                .await
                .filter(|(t, href)| t == title && *href == url)
        })
        .await;
        assert_eq!(seen, Some((title.to_owned(), url.clone())));
        let registered = support::eventually(Duration::from_secs(10), || async {
            runtime.target(&tab.target_id).filter(|t| t.url == url)
        })
        .await;
        assert!(
            registered.is_some(),
            "the registry follows the tab to {url}"
        );
    }

    // A navigation the browser rejects is an error, not a silent success.
    let refused = page.navigate("http://127.0.0.1:1/unreachable.html").await;
    assert!(
        matches!(refused, Err(RuntimeError::NavigationFailed(ref text)) if text.contains("ERR_")),
        "{refused:?}"
    );
    assert_eq!(runtime.resyncs(), 0, "no events were missed");
    assert!(
        fixture
            .server
            .requests()
            .iter()
            .any(|r| r == "GET /second.html")
    );

    // Closing the tab ends the registry's session; the page then refuses calls
    // instead of sending them to a dead or reused session.
    fixture
        .cdp()
        .call(
            "Target.closeTarget",
            json!({ "targetId": tab.target_id.as_str() }),
            None,
            support::CALL,
        )
        .await
        .expect("close the tab");
    let detached = support::eventually(Duration::from_secs(10), || async {
        match page
            .call("Runtime.evaluate", json!({ "expression": "1" }))
            .await
        {
            Err(RuntimeError::Detached) => Some(()),
            _ => None,
        }
    })
    .await;
    assert!(detached.is_some(), "a closed tab's page refuses calls");
    assert!(runtime.target(&tab.target_id).is_none());

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}
