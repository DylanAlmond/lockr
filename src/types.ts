export interface Vault {
  id: string;
  name: string;
  services: Service[];
}

export interface Service {
  id: string;
  name: string;
  accounts: Account[];
}

export interface Account {
  id: string;
  display_name: string | null;
  username: string;
  email: string | null;
  // favourite: boolean;
  // tags: string[];
  secret: AccountSecret;
  created_at: string;
  updated_at: string;
}

export interface AccountSecret {
  id: string;
  password: string;
}
