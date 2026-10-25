const MAX_BACKGROUND_BYTES = 16 * 1024 * 1024;
const MAX_BACKGROUND_URL_LENGTH = Math.ceil(MAX_BACKGROUND_BYTES / 3) * 4 + 64;

/** Decode the bounded image response without putting its bytes in a CSS value. */
export const decodeBackgroundImage = (value: string): { mime: string; bytes: Uint8Array<ArrayBuffer> } => {
  if (value.length > MAX_BACKGROUND_URL_LENGTH) throw new Error("CustomBackgroundInvalidImage");
  const prefix = /^data:(image\/(?:png|jpeg|gif|webp));base64,/.exec(value);
  if (!prefix) throw new Error("CustomBackgroundInvalidImage");
  const encoded = value.slice(prefix[0].length);
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded)) throw new Error("CustomBackgroundInvalidImage");
  let binary: string;
  try { binary = atob(encoded); }
  catch { throw new Error("CustomBackgroundInvalidImage"); }
  if (binary.length === 0 || binary.length > MAX_BACKGROUND_BYTES) throw new Error("CustomBackgroundInvalidImage");
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return { mime: prefix[1], bytes };
};
