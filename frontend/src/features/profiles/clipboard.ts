// ============================================================
// src/features/profiles/clipboard.ts
// Clipboard read/write via the Web Clipboard API. The Android WebUI runs in the
// manager webview / a browser, both of which expose `navigator.clipboard`.
// ============================================================

/** Write text to the clipboard. Returns true on success. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Read text from the clipboard, or null if unavailable / denied. */
export async function readText(): Promise<string | null> {
  try {
    return await navigator.clipboard.readText();
  } catch {
    return null;
  }
}
