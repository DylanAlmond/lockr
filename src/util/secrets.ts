/** Secret field keys of card items, mirrored from the backend's models.rs constants. */
export const SECRET_FIELD_CARD_NUMBER = 'card_number';
export const SECRET_FIELD_SECURITY_CODE = 'security_code';
export const SECRET_FIELD_PIN = 'pin';
export const SECRET_FIELD_PASSWORD = 'password';

/** Masked display for a card secret field with the value's structure: the
 * card number keeps the real last 4 digits when known, otherwise the whole
 * number is masked (including the final group). Returns null for absent
 * keys (rendered as 'No Value'). */
export function cardSecretMask(
  fieldKey: string,
  hasValue: boolean,
  last4: string | null
): string | null {
  if (!hasValue) return null;

  if (fieldKey === SECRET_FIELD_CARD_NUMBER) {
    return last4 ? `•••• •••• •••• ${last4}` : '•••• •••• •••• ••••';
  }
  return fieldKey === SECRET_FIELD_SECURITY_CODE ? '•••' : '••••';
}

/** Frontend mirror of the backend's `card_number_last4`: derive the last
 * four digits for a typed value (digits, whitespace and dashes only;
 * anything else, or fewer than four digits, yields null). */
export function cardNumberLast4(cardNumber: string): string | null {
  if (!cardNumber) return null;

  const isSeparator = (c: string) => (/\s/.test(c) && c !== '\n') || c === '-';
  if ([...cardNumber].some((c) => !/\d/.test(c) && !isSeparator(c))) return null;

  const digits = [...cardNumber].filter((c) => /\d/.test(c));
  if (digits.length < 4) return null;

  return digits.slice(-4).join('');
}
