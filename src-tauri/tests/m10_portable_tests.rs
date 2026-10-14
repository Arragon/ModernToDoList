//! M10 portable-hardening & release integration tests (INH-1117 ~ INH-1123).
//!
//! Covers, end-to-end through the library crate:
//! - RD-M10-024: deterministic release ZIP (identical SHA-256 across rebuilds).
//! - RD-M10-025~027: versioned Data-directory migration incl. the
//!   failure → delete index.db → rebuild-from-XML fallback.
//! - RD-M10-035~036: privacy-safe crash report + bug-report bundle export.
//! - RD-M10-037: license inventory generation from the real lockfiles
//!   (`#[ignore]`d generator writes LICENSES/ artifacts).
//! - RD-M10-038: EXE version metadata.
//! - RD-M10-040: release notes rendering.

use std::path::{Path, PathBuf};

use moderntodolist_lib::diagnostics::crashlog::{
    self, CrashReport, OperationTrail, StackFrame, SystemInfo,
};
use moderntodolist_lib::infrastructure::migration::{
    self, DataMigrationOutcome, MigrationError,
};
use moderntodolist_lib::release::licenses::LicenseInventory;
use moderntodolist_lib::release::notes;
use moderntodolist_lib::release::packaging::{self, ZipEntrySpec};
use moderntodolist_lib::release::version_metadata::VersionMetadata;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn write_sample_xml(dir: &Path, name: &str, tasks: usize) -> PathBuf {
    let path = dir.join(name);
    let mut body = String::new();
    body.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<TODOLIST PROJECTNAME=\"M10\" NEXTUNIQUEID=\"99\" FILEVERSION=\"43\" APPVER=\"9.0.14.0\" FILEFORMAT=\"12\">\n");
    for i in 1..=tasks {
        body.push_str(&format!(
            "<TASK ID=\"{i}\" TITLE=\"Task {i}\" POS=\"0\" POSSTRING=\"{i}\">\n</TASK>\n"
        ));
    }
    body.push_str("</TODOLIST>\n");
    std::fs::write(&path, body).unwrap();
    path
}

// ---------------------------------------------------------------------------
// RD-M10-024: deterministic release ZIP
// ---------------------------------------------------------------------------

#[test]
fn m10_024_release_zip_is_reproducible_byte_for_byte() {
    let entries = vec![
        ZipEntrySpec::file("ModernToDoList.exe", vec![0x4Du8; 4096]),
        ZipEntrySpec::file("Data/settings.json", br#"{"schemaVersion":2}"#.to_vec()),
        ZipEntrySpec::file(
            "LICENSES/THIRD_PARTY_NOTICES.md",
            b"# notices".to_vec(),
        ),
        ZipEntrySpec::dir("Data"),
        ZipEntrySpec::dir("LICENSES"),
    ];

    let a = packaging::build_deterministic_zip(&entries).unwrap();
    let b = packaging::build_deterministic_zip(&entries).unwrap();
    assert_eq!(a, b, "rebuild must be byte-identical");

    let hash_a = packaging::sha256_hex(&a);
    let hash_b = packaging::sha256_hex(&b);
    assert_eq!(hash_a, hash_b);
    assert_eq!(hash_a.len(), 64);

    // Input order must not matter.
    let mut reversed = entries.clone();
    reversed.reverse();
    let c = packaging::build_deterministic_zip(&reversed).unwrap();
    assert_eq!(packaging::sha256_hex(&c), hash_a);
}

#[test]
fn m10_024_release_artifact_naming() {
    assert_eq!(
        packaging::portable_zip_name("2.0.0"),
        "ModernToDoList-2.0.0-Portable-win-x64.zip"
    );

    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("stage");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("ModernToDoList.exe"), b"binary").unwrap();

    let out = tmp.path().join("release");
    let (zip_path, hash1) =
        packaging::build_portable_zip_from_dir(&staging, &out, "2.0.0").unwrap();
    let (_, hash2) = packaging::build_portable_zip_from_dir(&staging, &out, "2.0.0").unwrap();

    assert_eq!(
        zip_path.file_name().unwrap().to_string_lossy(),
        "ModernToDoList-2.0.0-Portable-win-x64.zip"
    );
    assert_eq!(hash1, hash2, "same staging dir => same SHA-256");
    assert!(out
        .join("ModernToDoList-2.0.0-Portable-win-x64.zip.sha256")
        .exists());
}

// ---------------------------------------------------------------------------
// RD-M10-025~027: Data migration + index rebuild fallback
// ---------------------------------------------------------------------------

#[test]
fn m10_025_data_dir_migration_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("Data");
    let xml = write_sample_xml(tmp.path(), "列表 tasks.xml", 3);

    // Legacy layout: settings without schemaVersion, an existing index.db.
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("settings.json"),
        r#"{"theme":"light","autosave_secs":60}"#,
    )
    .unwrap();
    {
        let conn = rusqlite::Connection::open(data.join("index.db")).unwrap();
        migration::run_migrations(&conn).unwrap();
    }

    let outcome = migration::migrate_or_rebuild(
        &data,
        "ws-m10",
        tmp.path(),
        &[("doc-m10".into(), xml.clone())],
    )
    .unwrap();
    assert!(matches!(
        outcome,
        DataMigrationOutcome::Migrated { applied: 3 }
    ));

    let state = migration::detect_data_dir_version(&data).unwrap();
    assert_eq!(state.data_dir_version, migration::CURRENT_DATA_DIR_VERSION);
    assert_eq!(
        state.settings_schema_version,
        migration::CURRENT_SETTINGS_SCHEMA_VERSION
    );

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["appearance"]["theme"], "light");
    assert_eq!(settings["session"]["autosaveSecs"], 60);
}

#[test]
fn m10_027_migration_failure_rebuilds_index_from_xml() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("Data");
    std::fs::create_dir_all(&data).unwrap();
    let xml = write_sample_xml(tmp.path(), "tasks.xml", 4);

    // Corrupt state: an index.db that a previous version left behind.
    {
        let conn = rusqlite::Connection::open(data.join("index.db")).unwrap();
        migration::run_migrations(&conn).unwrap();
    }

    let docs = vec![("doc-fb".to_string(), xml.clone())];
    let tasks = migration::rebuild_index_fallback(&data, "ws-fb", tmp.path(), &docs).unwrap();
    assert_eq!(tasks, 4);

    let conn = rusqlite::Connection::open(data.join("index.db")).unwrap();
    let count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_index WHERE document_id='doc-fb'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 4);
    // Schema migrations ran on the fresh database.
    assert_eq!(
        migration::get_current_version(&conn).unwrap(),
        moderntodolist_lib::infrastructure::schema::SCHEMA_VERSION
    );
}

#[test]
fn m10_027_failure_detection_is_typed() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("Data");
    std::fs::create_dir_all(&data).unwrap();
    // A settings.json that is not JSON blocks migration with a typed error.
    std::fs::write(data.join("settings.json"), "<<<not json>>>").unwrap();

    let err = migration::migrate_data_dir(&data).unwrap_err();
    assert!(
        matches!(err, MigrationError::MigrationFailed { .. }),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// RD-M10-035~036: crash report privacy + export
// ---------------------------------------------------------------------------

#[test]
fn m10_035_crash_report_hides_unicode_paths() {
    let mut trail = OperationTrail::new(10);
    trail.record(
        "open_document",
        Some(r"D:\用户文档\我的项目 副本\lists\秘密任务.xml"),
        5,
        false,
    );
    trail.record("save_document", Some(r"\\NAS\共享 文件夹\tasks.tdl"), 40, true);

    let report = CrashReport::capture(
        "2.0.0",
        "panic",
        "IO error while writing D:\\用户文档\\我的项目 副本\\Data\\index.db",
        vec![StackFrame::new("mtl::db::write")],
        &trail,
        SystemInfo::collect(None, None),
        "2026-01-15T10:00:00Z".to_string(),
    );

    let json = report.to_json();
    for forbidden in ["用户文档", "我的项目", "秘密任务", "共享 文件夹", "NAS", "index.db"] {
        assert!(
            !json.contains(forbidden),
            "privacy violation: report contains '{forbidden}'"
        );
    }
    assert!(json.contains(&crashlog::hash_path(
        r"D:\用户文档\我的项目 副本\lists\秘密任务.xml"
    )));
}

#[test]
fn m10_036_bug_report_bundle_export() {
    let tmp = tempfile::tempdir().unwrap();
    let report = CrashReport::capture(
        "2.0.0",
        "panic",
        "test crash",
        vec![StackFrame::new("mtl::main")],
        &OperationTrail::new(4),
        SystemInfo::collect(None, None),
        "2026-01-15T10:00:00Z".to_string(),
    );

    let bundle =
        crashlog::export_bug_report(tmp.path(), &report, &[("logs/app.txt", b"log".to_vec())])
            .unwrap();
    assert!(bundle.exists());

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&bundle).unwrap()).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&"crash-report.json".to_string()));
    assert!(names.contains(&"README.txt".to_string()));
    assert!(names.contains(&"logs/app.txt".to_string()));
}

// ---------------------------------------------------------------------------
// RD-M10-037: license inventory from the REAL lockfiles
// ---------------------------------------------------------------------------

#[test]
fn m10_037_real_lockfiles_parse_into_inventory() {
    let root = repo_root();
    let inv = LicenseInventory::from_lockfiles(
        &root.join("src-tauri").join("Cargo.lock"),
        &root.join("package-lock.json"),
    )
    .expect("both lockfiles must parse");

    // Direct dependencies must be present with curated SPDX licenses.
    let names: Vec<&str> = inv.rust_crates.iter().map(|c| c.name.as_str()).collect();
    for expected in ["serde", "tokio", "rusqlite", "quick-xml", "zip"] {
        assert!(names.contains(&expected), "crate {expected} missing from inventory");
    }
    let serde_dep = inv
        .rust_crates
        .iter()
        .find(|c| c.name == "serde")
        .unwrap();
    assert!(serde_dep.license.contains("MIT"));

    let npm_names: Vec<&str> = inv.npm_packages.iter().map(|p| p.name.as_str()).collect();
    assert!(npm_names.contains(&"vue"), "vue missing from npm inventory");

    let md = inv.render_rust_markdown();
    assert!(md.contains("| serde |"));
}

/// Regenerates the LICENSES/ notice files from the real lockfiles.
/// Run explicitly: `cargo test --test m10_portable_tests -- --ignored`
#[test]
#[ignore = "writes LICENSES/ artifacts into the repository"]
fn m10_037_generate_license_files() {
    let root = repo_root();
    let inv = LicenseInventory::from_lockfiles(
        &root.join("src-tauri").join("Cargo.lock"),
        &root.join("package-lock.json"),
    )
    .unwrap();
    inv.write_license_files(&root.join("LICENSES")).unwrap();
    assert!(root.join("LICENSES/RUST_DEPENDENCIES.md").exists());
    assert!(root.join("LICENSES/NPM_DEPENDENCIES.md").exists());
    assert!(root.join("LICENSES/THIRD_PARTY_NOTICES.md").exists());
}

// ---------------------------------------------------------------------------
// RD-M10-038 / RD-M10-040: version metadata + release notes
// ---------------------------------------------------------------------------

#[test]
fn m10_038_version_metadata_for_exe() {
    let m = VersionMetadata::v2_0_0();
    m.validate().unwrap();
    assert_eq!(m.file_version, "2.0.0.0");
    assert_eq!(m.product_version, "2.0.0");
    assert!(!m.company.is_empty());

    let snippet: serde_json::Value =
        serde_json::from_str(&m.render_tauri_conf_snippet()).unwrap();
    assert_eq!(snippet["bundle"]["windows"]["fileVersion"], "2.0.0.0");
}

#[test]
fn m10_040_release_notes_render_2_0_0_changelog() {
    let md = notes::release_notes_2_0_0().render_markdown();
    assert!(md.contains("# ModernToDoList 2.0.0 — Release Notes"));
    assert!(md.contains("## New Features"));
    assert!(md.contains("ModernToDoList-2.0.0-Portable-win-x64.zip"));

    let template = notes::ReleaseNotes::template("X.Y.Z", "2026-01-01").render_markdown();
    assert!(template.contains("## Breaking Changes"));
    assert!(template.contains("## Known Issues"));
}
