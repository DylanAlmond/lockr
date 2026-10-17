/** Secret field keys of card items, mirrored from the backend's models.rs constants. */
export const SECRET_FIELD_CARD_NUMBER = 'card_number';
export const SECRET_FIELD_SECURITY_CODE = 'security_code';
export const SECRET_FIELD_PIN = 'pin';
export const SECRET_FIELD_PASSWORD = 'password';

/** The secret keys cardSecretMask knows how to mask; account passwords are
 * masked as ACCOUNT_PASSWORD_MASK by the account components instead. */
export type CardSecretKey = 'card_number' | 'security_code' | 'pin';

/** Account password structural mask. Doubles as the copy-gating sentinel
 * in AccountField: a display value equal to it means "mask only, nothing
 * real to copy". */
export const ACCOUNT_PASSWORD_MASK = '••••••••••••••••';

/** Masked display for a card secret field with the value's structure: the
 * card number keeps the real last 4 digits when known, otherwise the whole
 * number is masked (including the final group). Returns null for absent
 * keys (rendered as 'No Value'). */
export function cardSecretMask(
  fieldKey: CardSecretKey,
  hasValue: boolean,
  last4: string | null
): string | null {
  if (!hasValue) return null;

  if (fieldKey === SECRET_FIELD_CARD_NUMBER) {
    return last4 ? `•••• •••• •••• ${last4}` : '•••• •••• •••• ••••';
  }
  return fieldKey === SECRET_FIELD_SECURITY_CODE ? '•••' : '••••';
}

/** Frontend mirror of the backend's `card_number_last4` derivation in
 * src-tauri/src/models.rs (pinned by test_card_number_last4_derivation in
 * src-tauri/src/vault_manager.rs): digits, whitespace and dashes only;
 * anything else, or fewer than four digits, yields null. Keep in sync with
 * the backend when changing the tolerated-input rules. */
export function cardNumberLast4(cardNumber: string): string | null {
  if (!cardNumber) return null;

  const isSeparator = (c: string) => (/\s/.test(c) && c !== '\n') || c === '-';
  if ([...cardNumber].some((c) => !/\d/.test(c) && !isSeparator(c))) return null;

  const digits = [...cardNumber].filter((c) => /\d/.test(c));
  if (digits.length < 4) return null;

  return digits.slice(-4).join('');
}
