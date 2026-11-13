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

/** A single-use Pairing offer, rendered as a QR payload plus a typed fallback. */
export interface PairingOffer {
  session_id: string;
  source_device_id: string;
  qr_payload: string;
  typed_code: string;
  expires_at: string;
}

/** What the human on the source Device approves before any key material moves. */
export interface PairingApprovalPrompt {
  device_id: string;
  name: string;
  platform: string;
}

/** What the joining Device announces. Identity metadata only. */
export interface PairingRequest {
  device_id: string;
  name: string;
  platform: string;
  public_key: string;
  schema_version: number;
}

/** A scanned QR payload, or the 8-character fallback plus the Device it belongs to. */
export type PairingCode =
  | { kind: 'qr'; payload: string }
  | { kind: 'typed'; source_device_id: string; code: string };

/**
 * A wire frame for the already-authenticated pairing channel. The `transfer`
 * frame carries key material to the joining Device; it is never rendered (see
 * the Safe View Pattern) and never contains the master password.
 */
export type PairingMessage =
  | ({ kind: 'join' } & PairingRequest)
  | {
      kind: 'transfer';
      schema_version: number;
      shared_profile: {
        user_id: string;
        name: string;
        color: string;
        icon: string | null;
        devices: unknown;
        vaults: unknown;
      };
      secret_key: number[];
      /** The source's pre-checked Sync Set for the joining Device. Absent or
       * null means the default: every Vault in the shared registry. */
      sync_set?: string[] | null;
    };

/** One Vault in the device-local Sync Set. `syncing: false` keeps the local
 * copy and its contents; it only stops offering and requesting it. */
export interface SyncSetVault {
  vault_id: string;
  name: string | null;
  syncing: boolean;
}

export interface SyncSetView {
  vaults: SyncSetVault[];
}

/** A paired Device with last-sync state. Safe View: no key material. */
export interface PairedDevice {
  device_id: string;
  name: string;
  platform: string;
  admitted: boolean;
  revoked: boolean;
  last_sync_at: string | null;
  is_this_device: boolean;
}

/** The only fields discovery may reveal: an opaque id, an address, and
 * whether this User has paired it. */
export interface DiscoveredPeer {
  device_id: string;
  address: string;
  paired: boolean;
}

export interface SyncConflictSide {
  device: string;
  counter: number;
  /** Always null for a secret field. */
  value: unknown;
}

/** A conflict of one field. Vault metadata conflicts carry the Vault id too;
 * `scope` is `vault` and `item_id` is absent in that case. */
export interface SyncConflict {
  vault_id: string;
  scope: 'item' | 'vault';
  item_id?: string;
  display_name?: string | null;
  field_key: string;
  is_secret: boolean;
  sides: [SyncConflictSide, SyncConflictSide];
}

/** Same Item identity found live in more than one Vault after a sync. This is
 * the concurrent-opposite-move case; review it as a Duplicate. */
export interface DuplicateIdentityNote {
  item_id: string;
  vault_ids: string[];
}

export interface SyncReport {
  session_id: string;
  peer_device_id: string;
  vault_ids: string[];
  conflicts: SyncConflict[];
  duplicate_notes: DuplicateIdentityNote[];
}

export type SyncProgress =
  | { event: 'started'; peer_device_id: string; vault_ids: string[] }
  | { event: 'scope_agreed'; peer_device_id: string; vault_ids: string[] }
  | { event: 'sending_vault'; vault_id: string; bytes: number }
  | { event: 'receiving_vault'; vault_id: string; bytes: number }
  | { event: 'vault_merged'; vault_id: string }
  | { event: 'vault_conflicted'; vault_id: string; conflicts: number }
  | { event: 'complete'; vaults: number; conflicts: number };

export interface UnregisteredVault {
  vault_id: string;
  file_name: string;
}

/** Pick one side of a Conflict. A secret is chosen by side so its value never
 * has to be sent to the UI. */
export type ConflictChoice = 'local' | 'remote';

export interface ConflictResolutionResult {
  remaining: number;
}
