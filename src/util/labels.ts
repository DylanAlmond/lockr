/** The first eight characters of an id, for labels that must stay short. */
export function shortId(id: string): string {
  return id.slice(0, 8);
}

/** An Item candidate's label: its display name, or a short-id fallback. */
export function candidateLabel(candidate: {
  item_id: string;
  display_name?: string | null;
}): string {
  return candidate.display_name?.trim() || `Item ${shortId(candidate.item_id)}`;
}

/** A Vault's display label: its name, or a short-id fallback. */
export function vaultLabel(vault: { vault_id: string; name?: string | null }): string {
  return vault.name?.trim() || `Vault ${shortId(vault.vault_id)}`;
}

/** A Vault label looked up from a `vault_id -> name` map. */
export function vaultLabelById(vaultId: string, names: Record<string, string>): string {
  return names[vaultId] ?? `Vault ${shortId(vaultId)}`;
}
