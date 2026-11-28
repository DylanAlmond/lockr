/** Upload limits (docs/security/uploads.md). */
export const MAX_UPLOAD_BYTES = 8 * 1024 * 1024;
export const ACCEPTED_IMAGE_TYPES = [
  'image/png',
  'image/jpeg',
  'image/webp',
  'image/gif'
] as const;

/**
 * Process a standard uploaded image into a 128x128 base64 image.
 *
 * Standard uploads keep the existing "fill the canvas" behaviour.
 */
export async function processImageToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    if (!ACCEPTED_IMAGE_TYPES.includes(file.type as (typeof ACCEPTED_IMAGE_TYPES)[number])) {
      reject(new Error('Unsupported image type'));
      return;
    }

    if (file.size > MAX_UPLOAD_BYTES) {
      reject(new Error('Image is too large'));
      return;
    }

    const reader = new FileReader();

    reader.onload = (e) => {
      const img = new Image();

      img.onload = () => {
        const SIZE = 128;

        const canvas = document.createElement('canvas');
        canvas.width = SIZE;
        canvas.height = SIZE;

        const ctx = canvas.getContext('2d');

        if (!ctx) {
          reject(new Error('Failed to get canvas context'));
          return;
        }

        // Scale the image to fill the 128x128 canvas.
        // This preserves the existing behaviour for standard uploads.
        const scale = Math.max(
          SIZE / img.width,
          SIZE / img.height
        );

        const width = img.width * scale;
        const height = img.height * scale;

        const x = (SIZE - width) / 2;
        const y = (SIZE - height) / 2;

        ctx.drawImage(img, x, y, width, height);

        // Keep standard uploads as PNG.
        const base64 = canvas.toDataURL('image/png');

        resolve(base64);
      };

      img.onerror = () => {
        reject(new Error('Failed to load image'));
      };

      img.src = e.target?.result as string;
    };

    reader.onerror = () => {
      reject(new Error('Failed to read file'));
    };

    reader.readAsDataURL(file);
  });
}

/**
 * Trigger a file input dialog for image selection.
 *
 * @returns A promise that resolves to a File object or null if cancelled
 */
export function selectImageFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');

    input.type = 'file';
    input.accept = ACCEPTED_IMAGE_TYPES.join(',');

    input.onchange = (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];

      resolve(file || null);
    };

    input.oncancel = () => {
      resolve(null);
    };

    input.click();
  });
}