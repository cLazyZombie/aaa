use super::*;

#[test]
fn install_schema_and_typescript_are_current() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/install.schema.json");
    if std::env::var_os("AAA_UPDATE_INSTALL").is_some() {
        std::fs::write(&path, schema()).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(path).unwrap_or_default(),
        schema(),
        "AAA_UPDATE_INSTALL=1 cargo test -p aaa-proto install_schema"
    );
}

#[test]
fn public_and_private_fixtures_roundtrip_with_versioned_shapes() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../../docs/install/fixtures.json")).unwrap();
    for fixture in fixtures {
        let typed: InstallContract = serde_json::from_value(fixture).unwrap();
        let bytes = serde_json::to_vec(&typed).unwrap();
        let _: InstallContract = serde_json::from_slice(&bytes).unwrap();
        if let InstallContract::Result(result) = typed {
            assert!(result.valid_exit());
        }
    }
    let bad =
        serde_json::json!({"kind":"invocation","value":{"choices":{"role":"agent","api_token":"fake-private-value"}}});
    assert!(serde_json::from_value::<InstallContract>(bad).is_err());
    let mut result = serde_json::to_value(InstallResult::invalid()).unwrap();
    result["schema"] = serde_json::json!("aaa.install.v2");
    assert!(serde_json::from_value::<InstallResult>(result).is_err());
    let mut claims = serde_json::json!({"approved":true});
    assert!(serde_json::from_value::<InstallAuthorization>(claims.take()).is_err());
}

#[test]
fn status_exit_contract_rejects_false_success_and_private_fields() {
    let mut r = InstallResult::invalid();
    assert!(r.valid_exit());
    for code in [0, 3, 130, 255] {
        r.exit_code = code;
        assert!(!r.valid_exit());
    }
    r.status = InstallStatus::NeedsUser;
    r.exit_code = 3;
    assert!(!r.valid_exit());
    r.needs_user.push(InstallNeedsUser {
        code: "first_device_pairing".into(),
        target: "browser".into(),
        message: "첫 기기를 연결하세요".into(),
    });
    assert!(r.valid_exit());
    let mut public = serde_json::to_value(&r).unwrap();
    public["pairing_code"] = serde_json::json!("fake-private-value");
    assert!(serde_json::from_value::<InstallResult>(public).is_err());
    let mut stream = serde_json::to_value(InstallStreamItem::Result {
        schema: InstallEventSchema::V1,
        installation_id: None,
        seq: 1,
        result: Box::new(r),
    })
    .unwrap();
    stream["type"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<InstallStreamItem>(stream).is_err());
}

#[test]
fn expected_arch_uses_the_shared_ssh_spelling() {
    let arch: InstallArch = serde_json::from_str("\"x86_64\"").expect("t-0471/t-0472 SSH arch");
    assert_eq!(arch, InstallArch::X86_64);
    assert_eq!(serde_json::to_string(&arch).unwrap(), "\"x86_64\"");
    assert!(serde_json::from_str::<InstallArch>("\"x86-64\"").is_err());
}
