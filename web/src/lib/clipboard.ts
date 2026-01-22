export async function copyText(text: string): Promise<void> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch (error) {
      if (typeof document === 'undefined') {
        throw error;
      }
    }
  }

  if (typeof document === 'undefined') {
    throw new Error('Clipboard API not available');
  }

  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.left = '-9999px';
  textarea.style.top = '0';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.focus();
  textarea.select();

  let succeeded = false;
  try {
    succeeded = document.execCommand('copy');
  } catch (error) {
    document.body.removeChild(textarea);
    throw error;
  }

  document.body.removeChild(textarea);

  if (!succeeded) {
    throw new Error('Fallback copy failed');
  }
}
