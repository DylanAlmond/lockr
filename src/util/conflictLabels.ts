/** Human labels for the wire field keys that can appear in a Conflict. */
const FIELD_LABELS: Record<string, string> = {
  display_name: 'Display name',
  username: 'Username',
  email: 'Email address',
  phone: 'Phone number',
  full_name: 'Full name',
  address_line1: 'Address line 1',
  address_line2: 'Address line 2',
  city: 'City',
  postal_code: 'Postal code',
  country: 'Country',
  cardholder_name: 'Cardholder name',
  brand: 'Card brand',
  expiry_month: 'Expiry month',
  expiry_year: 'Expiry year',
  title: 'Title',
  body: 'Note body',
  favourite: 'Favourite',
  tags: 'Tags',
  icon: 'Icon',
  color: 'Colour',
  name: 'Name'
};

/** A Conflict field's label, with a readable fallback for unknown keys. */
export function conflictFieldLabel(fieldKey: string): string {
  return (
    FIELD_LABELS[fieldKey] ??
    fieldKey
      .replace(/_/g, ' ')
      .replace(/^./, (letter) => letter.toUpperCase())
  );
}
