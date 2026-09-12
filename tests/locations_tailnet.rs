use fileblade::locations::{Connection, tailnet};
use serde_json::json;

#[test]
fn status_peers_are_candidates_even_when_online_and_never_gain_mount_capabilities() {
    let status = json!({"BackendState":"Running","Self":{"ID":"self"},"Peer":{
        "key-a":{"ID":"a","HostName":"Laptop","DNSName":"laptop.tail.test.","Online":true},
        "key-b":{"ID":"b","HostName":"NAS","TailscaleIPs":["fd7a:115c:a1e0::10"],"Online":false},
        "key-self":{"ID":"self","DNSName":"self.tail.test."}
    }});
    let candidates = tailnet::candidates(&status).unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].host, "laptop.tail.test");
    assert_eq!(candidates[1].host, "fd7a:115c:a1e0::10");
    assert_eq!(
        candidates[1].location.canonical_uri,
        "sftp://[fd7a:115c:a1e0::10]/"
    );
    for peer in candidates {
        assert_eq!(peer.location.connection, Connection::Disconnected);
        assert!(peer.location.capabilities.is_empty());
        assert!(peer.location.session_generation.is_empty());
        assert!(peer.location.local_representation.is_none());
    }
}

#[test]
fn discovery_rejects_inactive_or_unbounded_inventory_and_skips_unusable_hosts() {
    assert!(tailnet::candidates(&json!({"BackendState":"NeedsLogin","Peer":{}})).is_err());
    let peers: serde_json::Map<String, serde_json::Value> =
        (0..4097).map(|n| (n.to_string(), json!({}))).collect();
    assert!(tailnet::candidates(&json!({"BackendState":"Running","Peer":peers})).is_err());
    let candidates = tailnet::candidates(&json!({"BackendState":"Running","Peer":{
        "user":{"DNSName":"user:secret@host/path"},
        "option":{"DNSName":"-oProxyCommand=bad"},
        "unknown":{"DNSName":""},
        "fallback":{"ID":"valid","DNSName":"bad/name","TailscaleIPs":["100.64.0.8"],"HostName":"safe\nname"}
    }})).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].host, "100.64.0.8");
    assert_eq!(candidates[0].location.label, "safename");
}

#[path = "support/isolated.rs"]
mod isolated;

#[test]
fn saved_locations_round_trip_only_connection_fields_and_refuse_unknown_schema() {
    if !isolated::child(None) {
        return;
    }
    use fileblade::locations::{saved, sftp::Saved};
    use std::os::unix::fs::PermissionsExt;
    let entry = Saved {
        host: "peer.tail.test".into(),
        user: "user".into(),
        path: "/home/user/space #percent%".into(),
    };
    assert!(saved::read().unwrap().is_empty());
    saved::remember(&entry).unwrap();
    saved::remember(&entry).unwrap();
    assert_eq!(saved::read().unwrap(), vec![entry.clone()]);
    let path = fileblade::paths::config_dir().join("locations.json");
    let encoded: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(encoded["entries"][0].as_object().unwrap().len(), 3);
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    std::thread::scope(|scope| {
        for index in 0..8 {
            let entry = &entry;
            scope.spawn(move || {
                saved::remember(&Saved {
                    host: format!("peer-{index}.tail.test"),
                    ..entry.clone()
                })
                .unwrap();
            });
        }
    });
    assert_eq!(saved::read().unwrap().len(), 9);
    let mut refused = false;
    for index in 0..40 {
        let before = std::fs::read(&path).unwrap();
        if saved::remember(&Saved {
            host: format!("large-{index}.tail.test"),
            user: "user".into(),
            path: format!("/{}", "x".repeat(4095)),
        })
        .is_err()
        {
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(saved::read().is_ok());
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "oversized saved document must be refused before publication"
    );
    let unsupported = b"{\"schema\":99,\"entries\":[]}";
    std::fs::write(&path, unsupported).unwrap();
    assert!(saved::remember(&entry).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), unsupported);
}
