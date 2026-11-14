/** A retry never grants access: the caller must obtain and validate a fresh status. */
export function privacyStatusRecoveryDelay(error: unknown, attempt: number): number | null {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  if (!message.includes("PrivacyStatusRetryable:") || !Number.isInteger(attempt) || attempt < 0 || attempt >= 3) return null;
  return 500 * 2 ** attempt;
}
