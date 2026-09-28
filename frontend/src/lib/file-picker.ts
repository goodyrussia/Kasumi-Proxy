// ============================================================
// src/lib/file-picker.ts
// Web file picker for import fields (backup JSON, routing-rules JSON). Opens a
// transient <input type="file"> and resolves with the file's text, or null when
// the user cancels.
// ============================================================

export async function pickTextFile(accept = "application/json,.json"): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    // A cancel fires no event in most browsers; resolve null on window refocus.
    const cleanup = () => {
      window.removeEventListener("focus", onFocus);
      input.remove();
    };
    const onFocus = () => {
      // Give the change event a tick to win the race on browsers that fire both.
      setTimeout(() => {
        cleanup();
        resolve(null);
      }, 300);
    };
    input.addEventListener("change", () => {
      const file = input.files?.[0];
      if (!file) {
        cleanup();
        resolve(null);
        return;
      }
      file
        .text()
        .then((text) => {
          cleanup();
          resolve(text);
        })
        .catch(() => {
          cleanup();
          resolve(null);
        });
    });
    document.body.appendChild(input);
    window.addEventListener("focus", onFocus);
    input.click();
  });
}
