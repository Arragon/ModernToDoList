//! Diagnostics for ModernToDoList 2.0 (RD-M10-034, RD-M10-035~036).
//!
//! - [`crashlog`]: structured, privacy-safe crash reports and the
//!   user-submittable bug-report export bundle.
//! - The antivirus false-positive checklist below is the machine-readable
//!   companion of `docs/release/ANTIVIRUS_FALSE_POSITIVES.md`; the app can
//!   surface it to users verbatim.

pub mod crashlog;

use serde::Serialize;

#[allow(unused_imports)]
pub use crashlog::{
    export_bug_report, hash_path, sanitize_message, CrashReport, OperationRecord, OperationTrail,
    SystemInfo,
};

/// Severity of a mitigation/checklist item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecklistSeverity {
    /// Must be done before every release.
    Required,
    /// Strongly recommended.
    Recommended,
    /// Optional, situational.
    Optional,
}

/// One item of the antivirus investigation/mitigation checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub severity: ChecklistSeverity,
    /// Phase in the release process where the item applies.
    pub phase: String,
}

/// Machine-readable antivirus false-positive checklist (INH-1120).
///
/// Mirrors `docs/release/ANTIVIRUS_FALSE_POSITIVES.md`. Serialize with
/// serde_json to expose it to the frontend or to attach it to bug reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntivirusChecklist {
    pub version: String,
    /// Common heuristic triggers observed in Tauri/Electron apps.
    pub known_triggers: Vec<String>,
    /// Ordered investigation process when a user reports an AV block.
    pub investigation_process: Vec<String>,
    /// Mitigation checklist items.
    pub mitigations: Vec<ChecklistItem>,
    /// Code-signing policy summary.
    pub code_signing_policy: Vec<String>,
}

/// The canonical AV false-positive checklist for the 2.0 release line.
pub fn antivirus_false_positive_checklist() -> AntivirusChecklist {
    AntivirusChecklist {
        version: "2.0.0".to_string(),
        known_triggers: vec![
            "Unsigned or newly-signed executables with low reputation (SmartScreen 'unknown publisher')".into(),
            "Tauri/Electron bootstrap patterns: small EXE that spawns or loads a WebView2 child process".into(),
            "Self-contained portable apps that create/write files next to their own EXE (Data/ folder)".into(),
            "Packed/compressed release ZIPs distributed via direct download without a store reputation history".into(),
            "File-watcher behavior (notify crate / ReadDirectoryChangesW) misread as ransomware-like scanning".into(),
            "Rust panic=abort + stripped symbols producing unusual crash signatures".into(),
            "Auto-start entries or single-instance mutex creation flagged as persistence behavior".into(),
        ],
        investigation_process: vec![
            "Record the exact AV product, version, detection name and the flagged file's SHA-256 (from the .sha256 sidecar)".into(),
            "Confirm the user's ZIP/EXE hash matches the official release hash — rule out a tampered download".into(),
            "Check the vendor's public detection lookup (VirusTotal with the file hash) for which engines flag it".into(),
            "Reproduce on a clean VM with the same AV product and default settings".into(),
            "Determine whether the trigger is static (signature/heuristic on the EXE) or behavioral (write/watch next to EXE)".into(),
            "File a false-positive report with the vendor; include hash, detection name, download URL and a description of legitimate behavior".into(),
            "Track the vendor case ID in Linear (INH project) until the detection is withdrawn in a definition update".into(),
        ],
        mitigations: vec![
            ChecklistItem {
                id: "AV-01".into(),
                title: "Publish SHA-256 for every artifact".into(),
                detail: "Ship the .sha256 sidecar next to the ZIP so users can verify integrity before reporting false positives.".into(),
                severity: ChecklistSeverity::Required,
                phase: "release".into(),
            },
            ChecklistItem {
                id: "AV-02".into(),
                title: "Code-sign the EXE".into(),
                detail: "Sign ModernToDoList.exe with an OV/EV Authenticode certificate; EV builds SmartScreen reputation fastest. See policy below.".into(),
                severity: ChecklistSeverity::Required,
                phase: "release".into(),
            },
            ChecklistItem {
                id: "AV-03".into(),
                title: "Avoid packers/compressors on the EXE".into(),
                detail: "Do not run UPX or similar over the binary; packers are a dominant heuristic trigger. The ZIP container provides sufficient size reduction.".into(),
                severity: ChecklistSeverity::Required,
                phase: "build".into(),
            },
            ChecklistItem {
                id: "AV-04".into(),
                title: "Keep deterministic builds".into(),
                detail: "Reproducible ZIPs (RD-M10-024) mean one hash per release; reputation and vendor submissions stay valid across rebuilds.".into(),
                severity: ChecklistSeverity::Recommended,
                phase: "build".into(),
            },
            ChecklistItem {
                id: "AV-05".into(),
                title: "Submit to major vendors pre-release".into(),
                detail: "Proactively submit the signed RC binary to Microsoft (SmartScreen), and the top consumer AV vendors' false-positive portals.".into(),
                severity: ChecklistSeverity::Recommended,
                phase: "rc".into(),
            },
            ChecklistItem {
                id: "AV-06".into(),
                title: "Document user-side workarounds".into(),
                detail: "MANUAL_UPDATE.md and the website carry the 'add exclusion / allow anyway' steps plus the verification hash.".into(),
                severity: ChecklistSeverity::Recommended,
                phase: "docs".into(),
            },
            ChecklistItem {
                id: "AV-07".into(),
                title: "Restrict filesystem scope at runtime".into(),
                detail: "Only touch the portable Data/ directory and user-selected files; never enumerate unrelated drives. Reduces behavioral detections.".into(),
                severity: ChecklistSeverity::Recommended,
                phase: "code".into(),
            },
            ChecklistItem {
                id: "AV-08".into(),
                title: "Ship a plain-text manifest inside the ZIP".into(),
                detail: "Include README-PORTABLE.txt describing what the app writes and where, helping AV labs and users classify behavior.".into(),
                severity: ChecklistSeverity::Optional,
                phase: "release".into(),
            },
        ],
        code_signing_policy: vec![
            "All GA release binaries MUST be Authenticode-signed before publication; unsigned binaries are internal-build-only.".into(),
            "Sign both the EXE and the ZIP where the vendor supports it; timestamp the signature (RFC 3161) so it survives certificate expiry.".into(),
            "Private keys live on an HSM or in a cloud signing service — never in the repository or on build-agent disks.".into(),
            "The signing identity must match the `company` field in release version metadata (VersionMetadata v2.0.0).".into(),
            "If a signing certificate is compromised, revoke it, re-sign affected releases with a new certificate, and notify users via release notes.".into(),
            "Until a certificate is procured, releases MUST prominently document the SmartScreen warning and the SHA-256 verification step.".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checklist_is_complete_and_wellformed() {
        let c = antivirus_false_positive_checklist();
        assert_eq!(c.version, "2.0.0");
        assert!(c.known_triggers.len() >= 5);
        assert!(c.investigation_process.len() >= 5);
        assert!(c.mitigations.len() >= 6);
        assert!(c.code_signing_policy.len() >= 4);

        // Unique IDs, all required fields non-empty.
        let mut ids = std::collections::HashSet::new();
        for m in &c.mitigations {
            assert!(ids.insert(m.id.clone()), "duplicate id {}", m.id);
            assert!(!m.title.trim().is_empty());
            assert!(!m.detail.trim().is_empty());
            assert!(!m.phase.trim().is_empty());
        }
    }

    #[test]
    fn checklist_serializes_to_machine_readable_json() {
        let json = serde_json::to_string(&antivirus_false_positive_checklist()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["mitigations"].is_array());
        assert_eq!(v["mitigations"][0]["severity"], "required");
        assert!(v["codeSigningPolicy"].is_array());
    }
}
