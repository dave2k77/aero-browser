//! E01 §7.4 baseline: navigation targets and export names.

use aero_agent::policy::{UnsafeTarget, allowed_navigation, safe_export_basename};

#[test]
fn unsafe_schemes_are_rejected() {
    for (raw, why) in [
        ("javascript:alert(1)", UnsafeTarget::Scheme),
        ("data:text/html,<p>x</p>", UnsafeTarget::Scheme),
        ("file:///etc/passwd", UnsafeTarget::Scheme),
        ("ftp://supplier-a.test/", UnsafeTarget::Scheme),
        ("about:blank", UnsafeTarget::Scheme),
        ("chrome://settings", UnsafeTarget::Scheme),
        ("blob:https://supplier-a.test/0f1e", UnsafeTarget::Scheme),
        ("https://user@supplier-a.test/", UnsafeTarget::Credentials),
        (
            "https://user:secret@supplier-a.test/",
            UnsafeTarget::Credentials,
        ),
        (
            "https://:secret@supplier-a.test/",
            UnsafeTarget::Credentials,
        ),
        ("not a url", UnsafeTarget::Invalid),
        ("https://", UnsafeTarget::Invalid),
        ("", UnsafeTarget::Invalid),
    ] {
        assert_eq!(allowed_navigation(raw, false), Err(why), "{raw}");
        // Fixture permission widens addresses only, never schemes.
        assert_eq!(allowed_navigation(raw, true), Err(why), "{raw} (fixtures)");
    }
}

#[test]
fn private_and_local_hosts_need_network_permission() {
    for raw in [
        "http://127.0.0.1/",
        "http://127.8.9.10:8080/x",
        "http://10.1.2.3/",
        "http://172.16.0.1/",
        "http://172.31.255.255/",
        "http://192.168.1.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://0.0.0.0/",
        "http://0.1.2.3/",
        "http://100.64.0.1/",
        "http://100.127.255.254/",
        "http://255.255.255.255/",
        "http://224.0.0.1/",
        // Alternative IPv4 spellings the URL parser normalises.
        "http://2130706433/",
        "http://0x7f.1/",
        "http://017700000001/",
        "http://[::1]/",
        "http://[::]/",
        "http://[fc00::1]/",
        "http://[fd12:3456::1]/",
        "http://[fe80::1]/",
        "http://[ff02::1]/",
        // Private IPv4 hidden in IPv6 forms.
        "http://[::ffff:127.0.0.1]/",
        "http://[::ffff:10.0.0.1]/",
        "http://[::ffff:a9fe:a9fe]/",
        "http://[64:ff9b::a00:1]/",
        "http://[::127.0.0.1]/",
    ] {
        assert_eq!(
            allowed_navigation(raw, false),
            Err(UnsafeTarget::PrivateAddress),
            "{raw}"
        );
        assert!(allowed_navigation(raw, true).is_ok(), "{raw} (fixtures)");
    }
    for raw in [
        "http://localhost/",
        "http://LOCALHOST:3000/",
        "http://localhost./",
        "http://api.localhost/",
    ] {
        assert_eq!(
            allowed_navigation(raw, false),
            Err(UnsafeTarget::Localhost),
            "{raw}"
        );
        assert!(allowed_navigation(raw, true).is_ok(), "{raw} (fixtures)");
    }
}

#[test]
fn public_targets_are_parsed_and_normalised() {
    for (raw, expected) in [
        (
            "https://supplier-a.test/catalogue?q=1",
            "https://supplier-a.test/catalogue?q=1",
        ),
        (
            "HTTPS://Supplier-A.TEST:443/a/../b",
            "https://supplier-a.test/b",
        ),
        ("http://93.184.215.14/", "http://93.184.215.14/"),
        ("http://172.32.0.1/", "http://172.32.0.1/"),
        ("http://100.128.0.1/", "http://100.128.0.1/"),
        ("http://[2606:4700::1111]/", "http://[2606:4700::1111]/"),
        ("http://[64:ff9b::808:808]/", "http://[64:ff9b::808:808]/"),
        ("https://bücher.example/", "https://xn--bcher-kva.example/"),
        ("https://localhost.example/", "https://localhost.example/"),
    ] {
        assert_eq!(
            allowed_navigation(raw, false).map(|url| url.to_string()),
            Ok(expected.to_owned()),
            "{raw}"
        );
    }
}

#[test]
fn export_basenames_are_plain_names() {
    for good in [
        "report.csv",
        "supplier-comparison_v2.md",
        "a",
        &"x".repeat(128),
    ] {
        assert!(safe_export_basename(good), "{good}");
    }
    for bad in [
        "",
        ".",
        "..",
        "../report.csv",
        "dir/report.csv",
        "report.csv\0",
        "report .csv",
        "rapport-été.csv",
        "C:\\report.csv",
        &"x".repeat(129),
    ] {
        assert!(!safe_export_basename(bad), "{bad:?}");
    }
}
