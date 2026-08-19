use chrono::Utc;
use openairac_export_xplane::{
    INSTALL_JOURNAL, InstallFailpoints, InstallJournal, InstallPhase, install_layer,
    install_layer_with_failpoints, recover_interrupted,
};
use tempfile::tempdir;

fn build_dummy_staging(dir: &std::path::Path, cycle: &str, tag: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let fix_path = dir.join("earth_fix.dat");
    let nav_path = dir.join("earth_nav.dat");
    let awy_path = dir.join("earth_awy.dat");
    let manifest_path = dir.join("manifest.json");

    std::fs::write(
        &fix_path,
        format!("I\n1200 Version - {cycle} {tag}\nFIX1 K1 11 0 0 FIX1\n99\n"),
    )
    .unwrap();
    std::fs::write(
        &nav_path,
        format!("I\n1200 Version - {cycle} {tag}\n2 0 0 0 350 50 0.0 NAV1 ENRT K1 NAV1\n99\n"),
    )
    .unwrap();
    std::fs::write(
        &awy_path,
        format!("I\n1100 Version - {cycle} {tag}\nFIX1 K1 11 NAV1 K1 2 N 1 100 200 J1\n99\n"),
    )
    .unwrap();

    use sha2::Digest;
    let sha_fix = format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(&fix_path).unwrap())
    );
    let sha_nav = format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(&nav_path).unwrap())
    );
    let sha_awy = format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(&awy_path).unwrap())
    );

    let manifest = serde_json::json!({
        "generator": "openairac 1.0.0",
        "cycle": cycle,
        "build_date": "20260806",
        "generated_at": Utc::now().to_rfc3339(),
        "files": [
            { "name": "earth_fix.dat", "sha256": sha_fix, "rows": 1 },
            { "name": "earth_nav.dat", "sha256": sha_nav, "rows": 1 },
            { "name": "earth_awy.dat", "sha256": sha_awy, "rows": 1 }
        ],
        "allow_empty": false,
        "world_fingerprint": format!("{cycle}-{tag}")
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn test_redteam_installer_attacks() {
    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("Custom Data");
    std::fs::create_dir_all(&target_dir).unwrap();

    // Prepare staging for cycle 2608
    let staging_2608 = dir.path().join("staging_2608");
    build_dummy_staging(&staging_2608, "2608", "v1");

    // Prepare staging for cycle 2609
    let staging_2609 = dir.path().join("staging_2609");
    build_dummy_staging(&staging_2609, "2609", "v1");

    // 1. Happy path initial install of 2608
    let rep_2608 = install_layer(&staging_2608, &target_dir).unwrap();
    assert_eq!(rep_2608.cycle, "2608");
    assert!(target_dir.join("earth_fix.dat").exists());
    assert!(target_dir.join("earth_nav.dat").exists());
    assert!(target_dir.join("earth_awy.dat").exists());

    // 2. ATTACK: Target is a plain file instead of a directory must FAIL closed
    let file_target = dir.path().join("fake_target_file");
    std::fs::write(&file_target, b"NOT_A_DIR").unwrap();
    let res_file_target = install_layer(&staging_2608, &file_target);
    assert!(
        res_file_target.is_err(),
        "install into plain file must fail"
    );

    // 3. ATTACK: Crash during swap (failpoint during_swap) triggers rollback and preserves target
    let staging_crash = dir.path().join("staging_crash");
    build_dummy_staging(&staging_crash, "2609", "crash_test");
    let res_crash = install_layer_with_failpoints(
        &staging_crash,
        &target_dir,
        &InstallFailpoints {
            during_swap: true,
            ..Default::default()
        },
    );
    assert!(
        res_crash.is_err(),
        "during_swap failpoint must error and rollback"
    );
    let fix_content = std::fs::read_to_string(target_dir.join("earth_fix.dat")).unwrap();
    assert!(
        fix_content.contains("2608"),
        "rollback must restore previous 2608 files"
    );

    // 4. ATTACK: Truncated / corrupt journal must block install until recovered or cleaned
    let journal_path = target_dir.join(INSTALL_JOURNAL);
    std::fs::write(&journal_path, b"CORRUPTED_TRUNCATED_JSON_DATA").unwrap();
    let res_corrupt_journal = install_layer(&staging_2609, &target_dir);
    assert!(
        res_corrupt_journal.is_err(),
        "corrupt journal must block install"
    );
    std::fs::remove_file(&journal_path).unwrap();

    // 5. ATTACK: Hard crash simulation (leftover journal with backed-up files)
    let backup_dir = target_dir.join(".openairac_backup_crash_recovery_test");
    std::fs::create_dir_all(&backup_dir).unwrap();
    std::fs::write(
        backup_dir.join("earth_fix.dat"),
        "I\n1200 Version - 2608 BACKED UP\nFIX1 K1 11 0 0 FIX1\n99\n",
    )
    .unwrap();
    let simulated_journal = InstallJournal {
        operation_id: "crash-test-op".into(),
        cycle: "2609".into(),
        files: vec!["earth_fix.dat".into()],
        backup_dir,
        phase: InstallPhase::BackedUp,
    };
    std::fs::write(
        target_dir.join(INSTALL_JOURNAL),
        serde_json::to_string_pretty(&simulated_journal).unwrap(),
    )
    .unwrap();

    // recover_interrupted must detect the leftover journal, restore from backup, and clean journal
    let recovered = recover_interrupted(&target_dir).unwrap();
    assert!(
        recovered.is_some(),
        "interrupted install must be detected and recovered"
    );
    let fix_content = std::fs::read_to_string(target_dir.join("earth_fix.dat")).unwrap();
    assert!(
        fix_content.contains("2608 BACKED UP"),
        "recovery must restore backed-up files"
    );
    assert!(
        !target_dir.join(INSTALL_JOURNAL).exists(),
        "recovery must remove journal"
    );

    // 6. Upgrading from 2608 to 2609 succeeds
    let rep_upgrade = install_layer(&staging_2609, &target_dir).unwrap();
    assert_eq!(rep_upgrade.cycle, "2609");
    let fix_content = std::fs::read_to_string(target_dir.join("earth_fix.dat")).unwrap();
    assert!(fix_content.contains("2609"));

    // 7. Installing the same cycle again with fresh staging succeeds idempotently
    let staging_2609_repeat = dir.path().join("staging_2609_repeat");
    build_dummy_staging(&staging_2609_repeat, "2609", "repeat");
    let rep_idempotent = install_layer(&staging_2609_repeat, &target_dir).unwrap();
    assert_eq!(rep_idempotent.cycle, "2609");
}
