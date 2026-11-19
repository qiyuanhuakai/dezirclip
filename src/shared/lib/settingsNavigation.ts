export const PRIVACY_RULES_DESTINATION = "privacy_rules";
export interface SettingsNavigationRequest {
  readonly id: number;
  readonly destination: typeof PRIVACY_RULES_DESTINATION | "closed";
}

export function createSettingsNavigation() {
  let revision = 0;
  let pending: SettingsNavigationRequest | null = null;
  const listeners = new Set<() => void>();
  function navigate(destination: SettingsNavigationRequest["destination"]) {
    pending = { id: ++revision, destination };
    listeners.forEach(listener => listener());
  }
  return {
    request(payload: unknown): boolean {
      if (!payload || typeof payload !== "object" || !("destination" in payload)
        || payload.destination !== PRIVACY_RULES_DESTINATION) return false;
      navigate(PRIVACY_RULES_DESTINATION);
      return true;
    },
    close(): void {
      navigate("closed");
    },
    take(): SettingsNavigationRequest | null {
      const value = pending;
      pending = null;
      return value;
    },
    subscribe(listener: () => void): () => void {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
  };
}

export const settingsNavigation = createSettingsNavigation();
