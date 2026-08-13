/**
 * Process a standard uploaded image into a 128x128 base64 image.
 *
 * Standard uploads keep the existing "fill the canvas" behaviour.
 */
export async function processImageToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    if (!file.type.startsWith('image/')) {
      reject(new Error('File must be an image'));
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
 * Process a logo fetched from the Logo.dev API.
 *
 * The logo is:
 * - resized to fit within a smaller area
 * - centred in a 128x128 canvas
 * - surrounded by padding
 * - given a background colour sampled from the logo's corners
 * - exported as WebP to reduce the resulting base64 size
 */
async function processApiLogoToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    if (!file.type.startsWith('image/')) {
      reject(new Error('File must be an image'));
      return;
    }

    const reader = new FileReader();

    reader.onload = (e) => {
      const img = new Image();

      img.onload = () => {
        const SIZE = 128;

        // Controls how large the API logo is inside the 128x128 canvas.
        // 96 gives approximately 16px padding on each side for square logos.
        const LOGO_SIZE = 96;

        // Create a source canvas so we can sample the logo background.
        const sourceCanvas = document.createElement('canvas');
        sourceCanvas.width = img.width;
        sourceCanvas.height = img.height;

        const sourceCtx = sourceCanvas.getContext('2d', {
          willReadFrequently: true
        });

        if (!sourceCtx) {
          reject(new Error('Failed to get source canvas context'));
          return;
        }

        sourceCtx.drawImage(img, 0, 0);

        // Determine the background colour from the image corners.
        const backgroundColor = getBackgroundColor(
          sourceCtx,
          img.width,
          img.height
        );

        // Create the final 128x128 canvas.
        const canvas = document.createElement('canvas');
        canvas.width = SIZE;
        canvas.height = SIZE;

        const ctx = canvas.getContext('2d');

        if (!ctx) {
          reject(new Error('Failed to get canvas context'));
          return;
        }

        // Fill the padding/background.
        ctx.fillStyle = backgroundColor;
        ctx.fillRect(0, 0, SIZE, SIZE);

        // Scale the logo down while preserving its aspect ratio.
        const scale = Math.min(
          LOGO_SIZE / img.width,
          LOGO_SIZE / img.height
        );

        const width = img.width * scale;
        const height = img.height * scale;

        // Centre the logo.
        const x = (SIZE - width) / 2;
        const y = (SIZE - height) / 2;

        ctx.drawImage(img, x, y, width, height);

        // WebP produces a considerably smaller result than PNG.
        const base64 = canvas.toDataURL('image/webp', 0.9);

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
 * Estimate the background colour by sampling the four corners.
 *
 * This is used only for API logos.
 */
function getBackgroundColor(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number
): string {
  // Sample a small area in each corner rather than a single pixel.
  const sampleSize = Math.max(
    1,
    Math.min(
      20,
      Math.floor(Math.min(width, height) * 0.05)
    )
  );

  const positions: [number, number][] = [
    [0, 0],
    [width - sampleSize, 0],
    [0, height - sampleSize],
    [width - sampleSize, height - sampleSize]
  ];

  const pixels: number[][] = [];

  for (const [x, y] of positions) {
    const data = ctx.getImageData(
      Math.max(0, x),
      Math.max(0, y),
      sampleSize,
      sampleSize
    ).data;

    for (let i = 0; i < data.length; i += 4) {
      const alpha = data[i + 3];

      // Ignore transparent pixels.
      if (alpha > 200) {
        pixels.push([
          data[i],
          data[i + 1],
          data[i + 2]
        ]);
      }
    }
  }

  // If there was no usable background colour,
  // default to white.
  if (pixels.length === 0) {
    return '#ffffff';
  }

  // Average all sampled colours.
  const totals = pixels.reduce(
    (acc, [r, g, b]) => {
      acc.r += r;
      acc.g += g;
      acc.b += b;

      return acc;
    },
    {
      r: 0,
      g: 0,
      b: 0
    }
  );

  const r = Math.round(totals.r / pixels.length);
  const g = Math.round(totals.g / pixels.length);
  const b = Math.round(totals.b / pixels.length);

  return `rgb(${r}, ${g}, ${b})`;
}

/**
 * Fetch a logo from the Logo.dev API and convert it to
 * a 128x128 base64 WebP image.
 *
 * API logos receive special processing:
 * - smaller logo
 * - centred
 * - background-matched padding
 *
 * @param brandName - The brand name to fetch a logo for
 * @returns A promise that resolves to a base64 string or null if not found
 */
export async function fetchBrandLogoAsBase64(
  brandName: string
): Promise<string | null> {
  if (!brandName || brandName.trim().length === 0) {
    return null;
  }

  try {
    // Get API token from environment.
    const apiToken = import.meta.env.VITE_LOGO_API_TOKEN;

    if (!apiToken) {
      console.warn('Logo API token not configured in .env');
      return null;
    }

    // Fetch the logo from Logo.dev.
    const logoUrl =
      `https://img.logo.dev/name/${encodeURIComponent(brandName)}` +
      `?token=${apiToken}&format=png&retina=true`;

    const response = await fetch(logoUrl);

    if (!response.ok) {
      return null;
    }

    const blob = await response.blob();

    const file = new File(
      [blob],
      `${brandName}.png`,
      { type: 'image/png' }
    );

    // IMPORTANT:
    // API logos use the special processing function.
    // Standard uploaded images still use processImageToBase64().
    return await processApiLogoToBase64(file);
  } catch (error) {
    console.error(
      `Failed to fetch logo for ${brandName}:`,
      error
    );

    return null;
  }
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
    input.accept = 'image/*';

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