export interface User {
  id: string;
  name: string;
  color: string;
  icon: string;
  active_vault_id: string | null;
}

export interface Vault {
  id: string;
  name: string;
  color: string;
}

export interface Account {
  id: string;
  vault_id: string;

  display_name: string | null;
  username: string;
  email: string | null;

  favourite: boolean;
  tags: string[];
  icon: string | null;
  color: string;

  created_at: string;
  updated_at: string;
}

export interface ItemFilter {
  vault_id?: string | null;
  favourite_only?: boolean | null;
  tags?: string[] | null;
  search_query?: string | null;
}

export interface NoteItem {
  id: string;
  vault_id: string;

  display_name: string | null;
  favourite: boolean;
  tags: string[];
  icon: string | null;
  color: string;

  created_at: string;
  updated_at: string;

  title: string | null;
  body: string;
}

export interface IdentityItem {
  id: string;
  vault_id: string;

  display_name: string | null;
  favourite: boolean;
  tags: string[];
  icon: string | null;
  color: string;

  created_at: string;
  updated_at: string;

  full_name: string;
  email: string | null;
  phone: string | null;
  address_line1: string | null;
  address_line2: string | null;
  city: string | null;
  postal_code: string | null;
  country: string | null;
}

export interface CardItem {
  id: string;
  vault_id: string;

  display_name: string | null;
  favourite: boolean;
  tags: string[];
  icon: string | null;
  color: string;

  created_at: string;
  updated_at: string;

  cardholder_name: string;
  brand: string | null;
  expiry_month: number;
  expiry_year: number;

  /** Backend-derived; the full number lives in the secret store */
  card_number_last4: string | null;

  /** Which secret field keys have values (card_number, security_code, pin) */
  secrets_present: string[];
}

/** Items sent to the UI, tagged by kind. Safe View: no secret fields. */
export type SafeItem =
  | ({ kind: 'account' } & Account)
  | ({ kind: 'card' } & CardItem)
  | ({ kind: 'note' } & NoteItem)
  | ({ kind: 'identity' } & IdentityItem);

/** One secret field offered at item creation (cards; keys like card_number). */
export interface SecretInput {
  key: string;
  value: string;
}

/** Kind payloads for creating/updating items (wire format). Account items
 * carry their initial password as a `password` SecretInput alongside. */
export type NewItemData =
  | { kind: 'account'; username: string; email: string | null }
  | { kind: 'note'; title: string | null; body: string }
  | {
      kind: 'card';
      cardholder_name: string;
      brand: string | null;
      expiry_month: number;
      expiry_year: number;
    }
  | {
      kind: 'identity';
      full_name: string;
      email: string | null;
      phone: string | null;
      address_line1: string | null;
      address_line2: string | null;
      city: string | null;
      postal_code: string | null;
      country: string | null;
    };

export interface Entropy {
  /** Estimated guesses needed to crack the password */
  guesses: number;

  /** Order of magnitude of guesses */
  guesses_log10: number;

  /** Crack time estimations */
  crack_times: CrackTimes;

  /** Overall strength score */
  score: number;

  /** Feedback for improving weak passwords */
  feedback: Feedback | null;

  /** Patterns used during password analysis */
  sequence: Match[];

  /** Calculation duration */
  calc_time: Duration;
}

export interface CrackTimes {
  guesses: number;
}

export interface Feedback {
  warning: string | null;
  suggestions: string[];
}

export interface Duration {
  secs: number;
  nanos: number;
}

export interface Match {
  pattern: string;
}
