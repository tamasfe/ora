/**
 * Copies the text to the clipboard.
 *
 * The Clipboard API is only available in secure contexts,
 * e.g. it is missing when the UI is served over plain HTTP from a non-localhost host,
 * in which case the legacy `execCommand` approach is used.
 */
export async function copyToClipboard(text: string): Promise<void> {
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();

  try {
    if (!document.execCommand("copy")) {
      throw new Error("copying to the clipboard is not supported");
    }
  } finally {
    textarea.remove();
  }
}
