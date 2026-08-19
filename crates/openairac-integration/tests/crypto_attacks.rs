use chrono::Utc;
use openairac_bundle::{
    SigningKeyPair, TrustRoot, build_bundle, sign_bundle, validate_channel_artifact_path,
    verify_bundle_with_trust,
};
use openairac_model::{AiracCycle, CycleId, CycleStatus, SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn test_redteam_crypto_and_trust_attacks() {
    let dir = tempdir().unwrap();

    // Generate authority keys
    let auth_key = SigningKeyPair::generate();
    let auth_trust = auth_key.public_key();

    // Generate untrusted attacker key
    let attacker_key = SigningKeyPair::generate();

    // Create a valid mini-store
    let store = WorldStore::open_in_memory().unwrap();
    let now = Utc::now();
    let snap_id = SourceSnapshotId("faa:2608".into());
    store
        .insert_source_snapshot(&SourceSnapshot {
            id: snap_id.clone(),
            provider: "FAA".into(),
            dataset: "CIFP".into(),
            provider_revision: Some("2608".into()),
            airac_cycle: Some("2608".into()),
            effective_from: Some(now),
            effective_until: None,
            retrieved_at: now,
            source_uri: "http://test".into(),
            content_sha256: "0".repeat(64),
            license_id: None,
            license_notes: None,
            parser_version: "1.0.0".into(),
        })
        .unwrap();

    store
        .insert_cycle(&AiracCycle {
            id: CycleId("2608".into()),
            effective_from: Some(now),
            effective_until: None,
            status: CycleStatus::Active,
            source_uri: None,
            created_at: now,
            updated_at: now,
            notes: None,
        })
        .unwrap();

    // Build signed bundle
    let (_id_valid, bundle_valid) =
        build_bundle(&store, &dir.path().join("root_valid"), now).unwrap();
    sign_bundle(&bundle_valid, &auth_key).unwrap();

    // 1. ATTACK: Unsigned bundle must FAIL verification against trust root
    let (_id_unsigned, bundle_unsigned) =
        build_bundle(&store, &dir.path().join("root_unsigned"), now).unwrap();
    let res_unsigned = verify_bundle_with_trust(&bundle_unsigned, Some(&auth_trust));
    assert!(res_unsigned.is_err(), "unsigned bundle must fail closed");

    // 2. ATTACK: Bundle signed by UNTRUSTED attacker key must FAIL verification against authority trust root
    let (_id_attacker, bundle_attacker) =
        build_bundle(&store, &dir.path().join("root_attacker"), now).unwrap();
    sign_bundle(&bundle_attacker, &attacker_key).unwrap();
    let res_untrusted = verify_bundle_with_trust(&bundle_attacker, Some(&auth_trust));
    assert!(
        res_untrusted.is_err(),
        "bundle signed by untrusted key must be rejected"
    );

    // 3. ATTACK: Modified payload file inside signed bundle must FAIL integrity verification
    let (_id_tampered, bundle_tampered_payload) =
        build_bundle(&store, &dir.path().join("root_tampered_p"), now).unwrap();
    sign_bundle(&bundle_tampered_payload, &auth_key).unwrap();
    // Tamper with SQLite payload database
    std::fs::write(
        bundle_tampered_payload.join("payload.openairac.sqlite"),
        b"MALICIOUS CORRUPTED SQLITE",
    )
    .unwrap();
    let res_tampered = verify_bundle_with_trust(&bundle_tampered_payload, Some(&auth_trust));
    assert!(
        res_tampered.is_err(),
        "tampered payload file must fail hash/signature check"
    );

    // 4. ATTACK: Modified manifest.json with valid signature of original bundle must FAIL signature check
    let (_id_tampered_m, bundle_tampered_manifest) =
        build_bundle(&store, &dir.path().join("root_tampered_m"), now).unwrap();
    sign_bundle(&bundle_tampered_manifest, &auth_key).unwrap();
    // Tamper manifest
    let mut manifest_text =
        std::fs::read_to_string(bundle_tampered_manifest.join("manifest.json")).unwrap();
    manifest_text.push(' ');
    std::fs::write(
        bundle_tampered_manifest.join("manifest.json"),
        manifest_text,
    )
    .unwrap();
    let res_manifest = verify_bundle_with_trust(&bundle_tampered_manifest, Some(&auth_trust));
    assert!(
        res_manifest.is_err(),
        "tampered manifest must fail Ed25519 signature check"
    );

    // 5. ATTACK: Truncated / malformed signature file must FAIL closed
    let (_id_bad_sig, bundle_bad_sig) =
        build_bundle(&store, &dir.path().join("root_bad_sig"), now).unwrap();
    sign_bundle(&bundle_bad_sig, &auth_key).unwrap();
    std::fs::write(bundle_bad_sig.join("manifest.sig"), b"INVALID_BASE64_BYTES").unwrap();
    let res_bad_sig = verify_bundle_with_trust(&bundle_bad_sig, Some(&auth_trust));
    assert!(
        res_bad_sig.is_err(),
        "invalid signature format must fail closed"
    );

    // 6. ATTACK: Malformed public key in TrustRoot::from_base64 must FAIL
    let res_malformed_key = TrustRoot::from_base64("NOT_A_VALID_BASE64_KEY");
    assert!(
        res_malformed_key.is_err(),
        "malformed trust root must be rejected"
    );

    // 7. ATTACK: Path traversal validation (must reject escaping relative paths)
    assert!(!validate_channel_artifact_path(Path::new(
        "../../etc/passwd"
    )));
    assert!(!validate_channel_artifact_path(Path::new(
        "..\\..\\evil.oab"
    )));
    assert!(!validate_channel_artifact_path(Path::new(
        "subdir/../../../evil.oab"
    )));
    assert!(validate_channel_artifact_path(Path::new(
        "openairac-2608.oab"
    )));
    assert!(validate_channel_artifact_path(Path::new(
        "v1/bundles/openairac-2608.oab"
    )));
}
