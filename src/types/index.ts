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

export interface AccountSecret {
  id: string;
  password: string;
}

export interface AccountFilter {
  vault_id?: string | null;
  favourite_only?: boolean | null;
  tags?: string[] | null;
  search_query?: string | null;
}

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