//! OS clipboard access for secret copy (secret-lifetime R-S4).
//!
//! Secrets are written here in the core, never through IPC: the renderer only
//! learns whether the copy succeeded. After the TTL the clipboard is cleared
//! only if it still holds the copied value, so content the User copied
//! afterwards is never clobbered. Logout and window close use the same
//! compare-and-clear against the last value this module copied.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

use zeroize::Zeroizing;

/// How long a copied secret stays on the clipboard.
pub const CLIPBOARD_TTL: Duration = Duration::from_secs(30);

struct LastCopy {
    generation: u64,
    value: Zeroizing<String>,
}

fn last_copy() -> &'static Mutex<Option<LastCopy>> {
    static LAST_COPY: OnceLock<Mutex<Option<LastCopy>>> = OnceLock::new();
    LAST_COPY.get_or_init(|| Mutex::new(None))
}

/// Run `f` under the clipboard-state lock. A poisoned lock is recovered: the
/// state is two plain fields and remains usable.
fn with_last_copy<T>(f: impl FnOnce(&mut Option<LastCopy>) -> T) -> T {
    let mut guard = last_copy()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

fn next_generation() -> u64 {
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

/// Whether the timer for `this_generation` is still the active copy. A newer
/// copy resets the window, so an older timer must leave the clipboard alone.
fn should_clear(active_generation: Option<u64>, this_generation: u64) -> bool {
    active_generation == Some(this_generation)
}

/// Write `secret` to the clipboard and arm a compare-and-clear timer.
pub fn write_secret_and_arm_clear(secret: &str) -> Result<(), String> {
    let generation = next_generation();

    // The write and the tracked copy are one critical section so a newer copy
    // cannot have its clipboard content cleared by an older timer.
    with_last_copy(|last| -> Result<(), String> {
        write(secret)?;
        *last = Some(LastCopy {
            generation,
            value: Zeroizing::new(secret.to_string()),
        });
        Ok(())
    })?;

    // The timer holds its own copy for the comparison, zeroized when the
    // thread ends. Detached by design: it must outlive the copy command.
    let copied = Zeroizing::new(secret.to_string());

    thread::spawn(move || {
        thread::sleep(CLIPBOARD_TTL);

        with_last_copy(|last| {
            if should_clear(last.as_ref().map(|tracked| tracked.generation), generation) {
                clear_if_unchanged(copied.as_str());
                last.take();
            }
        });
    });

    Ok(())
}

/// Best-effort clipboard clear for session end and window close. Only clears
/// when the clipboard still holds the last secret this module copied.
pub fn clear() {
    with_last_copy(|last| {
        if let Some(copied) = last.take() {
            clear_if_unchanged(copied.value.as_str());
        }
    });
}

fn clear_if_unchanged(value: &str) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.get_text().ok().as_deref() == Some(value) {
            let _ = clipboard.set_text(String::new());
        }
    }
}

fn write(secret: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_text(secret)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // rust:clipboard-expiry — only the latest copy's timer may clear.
    #[test]
    fn test_clipboard_expiry_only_clears_the_latest_copy() {
        assert!(should_clear(Some(2), 2));
        assert!(
            !should_clear(Some(2), 1),
            "an older timer must not clear a newer copy"
        );
        assert!(
            !should_clear(None, 1),
            "a cleared tracker means there is nothing to clear"
        );
    }
}
