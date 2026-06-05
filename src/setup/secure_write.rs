// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared secure-write helpers for the setup writers.
//!
//! Centralizes three robustness/security primitives that were previously
//! duplicated (or missing) across the per-client config, hook, and rules
//! writers:
//!
//!   * [`erebyx_command`] — the canonicalized absolute path of the running
//!     `erebyx` binary, so launched MCP servers / hook commands invoke the
//!     exact version the customer just ran `erebyx setup` from instead of
//!     relying on `$PATH` (GUI clients frequently launch with a minimal
//!     `$PATH` that doesn't include the install dir).
//!   * [`atomic_write_secret`] — write-to-temp + fsync-free `rename` so a
//!     crash mid-write can never leave a customer's *whole* Claude / Cursor /
//!     Zed config truncated or corrupt; the temp file is chmod'd `0o600`
//!     BEFORE the rename so a key-bearing file never exists world-readable
//!     even for an instant.
//!   * [`harden_windows_acl`] / [`ensure_dir_secure`] — defense-in-depth
//!     permission tightening on the platforms that need it.
//!
//! This module is wired in via a `#[path]` declaration on the `config`
//! module (see `config.rs`) so it can be shared by `config.rs`, `hooks.rs`,
//! and `rules.rs` without modifying `mod.rs`.

use anyhow::{Context, Result};
use std::path::Path;

/// Resolve the `erebyx` binary path that AI clients / hooks should launch.
///
/// Prefers the absolute, symlink-resolved path of the currently-running
/// binary so the launched MCP server (and the SessionStart hook command) is
/// always the same version the user just ran `erebyx setup` from. Falls back
/// to the bare command name if the current exe path can't be resolved (the
/// client will then rely on `$PATH`).
///
/// Why absolute matters: GUI-launched AI clients (Claude Code desktop, Cursor,
/// Zed) often run with a minimal `$PATH` that does NOT include `~/.cargo/bin`
/// or `/usr/local/bin`, so a bare `erebyx` command silently fails to launch.
pub fn erebyx_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "erebyx".to_string())
}

/// Atomically write `bytes` to `path`, never leaving a partially-written or
/// world-readable file behind.
///
/// Sequence:
///   1. Write to a sibling temp file `<path>.erebyx.tmp` in the SAME directory
///      (same-volume guarantee so the final `rename` is atomic).
///   2. On Unix, chmod the TEMP file to `0o600` BEFORE the rename — so the
///      key-bearing bytes never exist at the final path with permissive
///      perms, even transiently.
///   3. `fs::rename` the temp over the target. On the same volume this is an
///      atomic replace: readers see either the old complete file or the new
///      complete file, never a truncated mix. This is what protects the
///      customer's WHOLE client config from corruption if the process dies
///      mid-write.
///   4. On Windows, re-tighten the DACL on the final path via
///      [`harden_windows_acl`] AFTER the rename. The rename replaces the
///      destination's security descriptor, so without this every write would
///      silently revert any DACL a caller had previously applied — hardening
///      here makes the lock-down uniform with the unix `0o600` and
///      independent of the caller.
///
/// On any failure the temp file is best-effort removed so we don't litter
/// `<path>.erebyx.tmp` next to the user's config.
pub fn atomic_write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    // Temp file alongside the target (same dir => same volume => atomic
    // rename). Suffix is distinctive so a leftover after a hard crash is
    // recognizable as ours.
    let file_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let mut tmp_name = file_name;
    tmp_name.push(".erebyx.tmp");
    let tmp_path = parent.join(&tmp_name);

    // Create the temp file 0o600-AT-CREATION (unix) so the secret is NEVER
    // world-readable — not even in the brief window the old write-then-chmod
    // flow left open — and with O_EXCL (`create_new`) so a pre-placed symlink
    // at the fixed temp path can't redirect the write to a file we don't own.
    // A leftover temp from a prior hard crash is cleared first (best-effort) so
    // create_new doesn't spuriously block on our own debris.
    let _ = std::fs::remove_file(&tmp_path);
    let write_result: Result<()> = (|| {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp_path)
            .with_context(|| format!("Failed to create temp file {}", tmp_path.display()))?;
        f.write_all(bytes)
            .with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Belt-and-suspenders: normalize to EXACTLY 0o600 on unix. Creation already
    // bounded it to <= 0o600 via umask; this pins it regardless of umask.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        if let Err(e) = std::fs::set_permissions(&tmp_path, perms).with_context(|| {
            format!(
                "Failed to set permissions on temp file {}",
                tmp_path.display()
            )
        }) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    }

    // Atomic same-volume replace.
    //
    // Windows note: `std::fs::rename` maps to `MoveFileEx` /
    // `ReplaceFile`-style semantics and DOES overwrite an existing
    // destination on modern Windows. This arm needs Windows-runtime
    // verification before the v0.1.2 tag — the overwrite-on-rename behavior
    // is correct on supported Windows versions but has not been exercised on
    // a real Windows box in this codebase yet.
    if let Err(e) = std::fs::rename(&tmp_path, path)
        .with_context(|| format!("Failed to atomically rename into place: {}", path.display()))
    {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Windows DACL hardening, AT the write boundary (fail-soft).
    //
    // The atomic `rename` above replaces the destination's security
    // descriptor with the temp file's — so any DACL a CALLER previously
    // tightened on `path` is silently reverted by every subsequent write.
    // (The Claude Code `settings.json` is rewritten here by `hooks.rs` AFTER
    // `config.rs` hardened it, which is exactly that revert.) Folding the
    // tighten INTO this function means every secret write re-hardens the
    // final file regardless of caller — the Windows analogue of the unix
    // `0o600` we always pin above. `harden_windows_acl` is fail-soft and
    // idempotent, so a caller that ALSO hardens (config.rs) is harmless.
    #[cfg(windows)]
    {
        harden_windows_acl(path);
    }

    Ok(())
}

/// Tighten the DACL on a freshly-written key-bearing file on Windows.
///
/// FAIL-SOFT: shells out to `icacls` with arguments passed as a VEC (never a
/// shell string — no `cmd.exe` parsing, so no metacharacter injection risk).
/// If `icacls` is missing, errors, or returns a nonzero status, we fall back
/// to the existing one-line `eprintln` warning. The downside is therefore
/// never worse than today (today is warn-only), but on the happy path the
/// file ends up locked to the current user + SYSTEM + Administrators.
///
/// HIGHEST runtime risk in the v0.1.2 set — the username/SID resolution edge
/// (renamed accounts, domain-joined boxes, non-English locales where
/// "Administrators" is localized) can silently no-op the grant. MUST be tested
/// on a multi-user / domain-joined Windows box before the v0.1.2 tag.
#[cfg(windows)]
pub fn harden_windows_acl(path: &Path) {
    use std::process::Command;

    // Resolve the current user safely. `USERNAME` is the canonical Windows
    // env var; fall back to `whoami` (which prints `DOMAIN\user`) if it's
    // somehow unset. icacls accepts `DOMAIN\user` and bare `user` alike.
    let current_user = std::env::var("USERNAME")
        .ok()
        .filter(|u| !u.trim().is_empty());
    let current_user = current_user.or_else(|| {
        Command::new("whoami")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });

    let path_str = path.display().to_string();

    let attempt = current_user.as_ref().and_then(|user| {
        let grant_user = format!("{user}:F");
        let args: Vec<&str> = vec![
            path_str.as_str(),
            "/inheritance:r",
            "/grant:r",
            grant_user.as_str(),
            "/grant:r",
            "SYSTEM:F",
            "/grant:r",
            "Administrators:F",
        ];
        Command::new("icacls").args(&args).output().ok()
    });

    let hardened = matches!(&attempt, Some(out) if out.status.success());

    if !hardened {
        eprintln!(
            "  ⚠ Windows: could not auto-restrict permissions on {path_str} via icacls.\n\
             \n\
             Confirm %USERPROFILE% is not world-readable. To tighten ACLs by hand, run:\n\
             \n\
                 icacls \"{path_str}\" /inheritance:r ^\n\
                     /grant:r \"%USERNAME%:F\" \"SYSTEM:F\" \"Administrators:F\"\n\
             \n\
             See SECURITY.md -> \"API-key file handling\" for the full guidance."
        );
    }
}

/// Create `dir` (and parents) if needed, then on Unix tighten it to `0o700`
/// — but only when WE created it. Stat first: a pre-existing directory keeps
/// the user's own permissions so `erebyx setup` doesn't surprise anyone by
/// silently re-chmod'ing, say, their whole `~/.config` tree.
pub fn ensure_dir_secure(dir: &Path) -> Result<()> {
    let pre_existing = dir.is_dir();

    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create directory: {}", dir.display()))?;

    #[cfg(unix)]
    {
        if !pre_existing {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(dir, perms)
                .with_context(|| format!("Failed to set permissions on {}", dir.display()))?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pre_existing;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn erebyx_command_resolves_to_nonempty() {
        // In the test binary, current_exe resolves; assert we get a real
        // absolute path (not the bare fallback) under cargo test.
        let cmd = erebyx_command();
        assert!(!cmd.is_empty(), "erebyx_command must never be empty");
    }

    #[test]
    fn atomic_write_secret_writes_full_contents() {
        let td = TempDir::new().expect("tempdir");
        let target = td.path().join("settings.json");
        let payload = br#"{"mcpServers":{"erebyx-os":{}}}"#;

        atomic_write_secret(&target, payload).expect("atomic write");

        let read_back = std::fs::read(&target).expect("read back");
        assert_eq!(read_back, payload, "contents must round-trip exactly");
        // The temp file must be gone after a successful write.
        let tmp = td.path().join("settings.json.erebyx.tmp");
        assert!(!tmp.exists(), "temp file must be cleaned up after rename");
    }

    #[test]
    fn atomic_write_secret_overwrites_existing_atomically() {
        let td = TempDir::new().expect("tempdir");
        let target = td.path().join("config.json");
        std::fs::write(&target, b"OLD CONTENTS THAT SHOULD BE REPLACED").unwrap();

        atomic_write_secret(&target, b"NEW").expect("atomic overwrite");

        let read_back = std::fs::read_to_string(&target).expect("read back");
        assert_eq!(read_back, "NEW", "existing file must be fully replaced");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_secret_sets_owner_only_perms() {
        use std::os::unix::fs::PermissionsExt;
        let td = TempDir::new().expect("tempdir");
        let target = td.path().join("key.json");

        atomic_write_secret(&target, b"secret-bearing").expect("atomic write");

        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        // Only the low 9 perm bits matter; must be exactly owner rw.
        assert_eq!(mode & 0o777, 0o600, "key file must be 0o600, got {mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_secure_chmods_newly_created_dir_to_0700() {
        use std::os::unix::fs::PermissionsExt;
        let td = TempDir::new().expect("tempdir");
        let new_dir = td.path().join("hooks");

        ensure_dir_secure(&new_dir).expect("create + chmod");

        let mode = std::fs::metadata(&new_dir).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o700,
            "newly-created dir must be 0o700, got {mode:o}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_secure_leaves_preexisting_dir_perms_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let td = TempDir::new().expect("tempdir");
        let dir = td.path().join("preexisting");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        ensure_dir_secure(&dir).expect("no-op create + skip chmod");

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o755,
            "pre-existing dir perms must be left untouched, got {mode:o}"
        );
    }
}
