//! Authentication compliance checks.
//!
//! Ids: `rust:password-policy`, `rust:login-rate-limit`, `rust:auth-logging`.
//! Each test name carries its checklist id in the form a failing run prints.

use std::path::PathBuf;

use lockr_lib::user_manager::{validate_master_password, UserManager};
use lockr_lib::VaultError;
use uuid::Uuid;

const STRONG_PW: &str = "correct horse battery staple";

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lockr_compliance_auth_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn rust_password_policy_rejects_short_and_weak_master_passwords() {
    assert!(validate_master_password("Alice", "short").is_err());
    assert!(
        validate_master_password("Alice", "passwordpassword").is_err(),
        "a predictable password must not pass the score floor"
    );
    assert!(validate_master_password("Alice", STRONG_PW).is_ok());
}

#[test]
fn rust_login_rate_limit_refuses_the_correct_password_while_blocked() {
    let dir = temp_dir();
    let mut manager = UserManager::new(dir.clone()).unwrap();
    let user = manager
        .register("Alice".to_string(), STRONG_PW)
        .unwrap();

    for _ in 0..5 {
        let result = manager.login(&user.id, "wrong password");
        assert!(
            matches!(result, Err(VaultError::AuthenticationFailed)),
            "got {result:?}"
        );
    }

    // The correct password must wait out the throttle too.
    let blocked = manager.login(&user.id, STRONG_PW);
    assert!(
        matches!(blocked, Err(VaultError::TooManyAttempts(_))),
        "got {blocked:?}"
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn rust_auth_logging_records_success_and_failure_without_secrets() {
    let dir = temp_dir();
    let mut manager = UserManager::new(dir.clone()).unwrap();
    let user = manager
        .register("Alice".to_string(), STRONG_PW)
        .unwrap();

    let wrong = "wrong password";
    manager.login(&user.id, wrong).unwrap_err();
    manager.login(&user.id, STRONG_PW).unwrap();

    let log = std::fs::read_to_string(dir.join("auth.log")).unwrap();
    assert!(log.contains("registered"));
    assert!(log.contains("login_failed"));
    assert!(log.contains("login_success"));
    assert!(!log.contains(STRONG_PW), "the log must never contain a password");
    assert!(!log.contains(wrong), "the log must never contain an attempt");

    std::fs::remove_dir_all(dir).ok();
}
