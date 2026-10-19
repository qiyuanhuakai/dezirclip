/** Read the message of an IPC or JavaScript error without dumping arbitrary objects. */
export const errorMessage = (reason: unknown): string => {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "object" && reason !== null && "message" in reason && typeof reason.message === "string") {
    return reason.message;
  }
  return "";
};
