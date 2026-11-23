/** Credential operations target the opaque key-provider ID, never the subscription ID or the secret. */
export interface KeyProvider { provider: string; name: string; key_saved: boolean; can_remove?: boolean }

export function canRemoveKey(key: KeyProvider) {
  return key.key_saved === true && key.can_remove === true && /^api-key:.+/.test(key.provider);
}

/** Only report success when the engine's refreshed inventory confirms the saved key is gone. */
export async function removeSavedKey(key: KeyProvider, confirm: () => Promise<boolean>,
  remove: (provider: string) => Promise<unknown>, reload: () => Promise<KeyProvider[]>, current: () => boolean) {
  if (!canRemoveKey(key) || !current() || !await confirm() || !current()) return false;
  await remove(key.provider);
  if (!current()) return false;
  const keys = await reload();
  if (!current()) return false;
  if (!keys.some(k => k.provider === key.provider && k.key_saved === false)) {
    throw new Error('Dial could not verify that the saved key was removed.');
  }
  return true;
}
