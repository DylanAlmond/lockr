//! Per-field bounds for strings crossing IPC (F-17).
//!
//! The renderer is treated as untrusted: every free-text field is bounded here
//! so an oversized value is rejected with a validation error instead of being
//! persisted. Bounds are characters, not bytes, so a limit reads as the User
//! sees it.

use crate::{NewItemData, VaultError};

pub const MAX_NAME: usize = 256;
pub const MAX_DISPLAY_NAME: usize = 256;
pub const MAX_TITLE: usize = 512;
pub const MAX_BODY: usize = 256 * 1024;
pub const MAX_USERNAME: usize = 256;
pub const MAX_EMAIL: usize = 320;
pub const MAX_PHONE: usize = 64;
pub const MAX_ADDRESS_LINE: usize = 256;
pub const MAX_CITY: usize = 128;
pub const MAX_POSTAL_CODE: usize = 32;
pub const MAX_COUNTRY: usize = 64;
pub const MAX_BRAND: usize = 64;
pub const MAX_CARDHOLDER: usize = 256;
pub const MAX_TAG: usize = 64;
pub const MAX_TAGS: usize = 50;
pub const MAX_COLOR: usize = 32;
pub const MAX_SECRET: usize = 4096;

fn check(field: &str, value: &str, max: usize) -> Result<(), VaultError> {
    if value.chars().count() > max {
        return Err(VaultError::Validation(format!(
            "{field} must be at most {max} characters"
        )));
    }

    Ok(())
}

pub fn text(field: &str, value: &str, max: usize) -> Result<(), VaultError> {
    check(field, value, max)
}

pub fn optional(field: &str, value: &Option<String>, max: usize) -> Result<(), VaultError> {
    if let Some(value) = value {
        check(field, value, max)?;
    }

    Ok(())
}

pub fn color(value: &str) -> Result<(), VaultError> {
    check("color", value, MAX_COLOR)
}

pub fn tags(tags: &[String]) -> Result<(), VaultError> {
    if tags.len() > MAX_TAGS {
        return Err(VaultError::Validation(format!(
            "tags must contain at most {MAX_TAGS} entries"
        )));
    }

    for tag in tags {
        check("tag", tag, MAX_TAG)?;
    }

    Ok(())
}

pub fn secret_value(field: &str, value: &str) -> Result<(), VaultError> {
    check(field, value, MAX_SECRET)
}

pub fn item_data(data: &NewItemData) -> Result<(), VaultError> {
    match data {
        NewItemData::Account(account) => {
            check("username", &account.username, MAX_USERNAME)?;
            optional("email", &account.email, MAX_EMAIL)
        }
        NewItemData::Note(note) => {
            optional("title", &note.title, MAX_TITLE)?;
            check("note body", &note.body, MAX_BODY)
        }
        NewItemData::Card(card) => {
            check("cardholder name", &card.cardholder_name, MAX_CARDHOLDER)?;
            optional("brand", &card.brand, MAX_BRAND)
        }
        NewItemData::Identity(identity) => {
            check("full name", &identity.full_name, MAX_NAME)?;
            optional("email", &identity.email, MAX_EMAIL)?;
            optional("phone", &identity.phone, MAX_PHONE)?;
            optional("address line", &identity.address_line1, MAX_ADDRESS_LINE)?;
            optional("address line", &identity.address_line2, MAX_ADDRESS_LINE)?;
            optional("city", &identity.city, MAX_CITY)?;
            optional("postal code", &identity.postal_code, MAX_POSTAL_CODE)?;
            optional("country", &identity.country, MAX_COUNTRY)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_bound_counts_characters() {
        text("name", &"é".repeat(MAX_NAME), MAX_NAME).unwrap();
        assert!(text("name", &"é".repeat(MAX_NAME + 1), MAX_NAME).is_err());
    }

    #[test]
    fn test_tags_bound_count_and_length() {
        tags(&vec!["ok".to_string(); MAX_TAGS]).unwrap();
        assert!(tags(&vec!["ok".to_string(); MAX_TAGS + 1]).is_err());
        assert!(tags(&["x".repeat(MAX_TAG + 1)]).is_err());
    }

    #[test]
    fn test_optional_fields_are_bounded() {
        optional("email", &Some("a".repeat(MAX_EMAIL + 1)), MAX_EMAIL).unwrap_err();
        optional("email", &None, MAX_EMAIL).unwrap();
    }
}
