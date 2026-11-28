# Image uploads

What the icon uploader accepts, what it produces, and where the limit is
enforced. Decision record for F-21.

## 1. Input limits

| Limit | Value |
|---|---|
| Formats | PNG, JPEG, WebP, GIF |
| Maximum input size | 8 MiB |
| Maximum output | 128 × 128 px PNG `data:` URL |

The file picker filters to the four formats, and `processImageToBase64` rejects
any other type or a file larger than 8 MiB **before decoding**. The file never
leaves the machine: it is read with `FileReader` in the renderer.

## 2. Processing

The picked file is decoded by the webview's platform image decoder, drawn onto
a 128 × 128 canvas with a 1:1 cover fit, and exported as
`data:image/png;base64,…`. Only that string crosses IPC, as the `icon` field of
an item or profile update.

SVG is not accepted by the picker and is not in the format allowlist, so an
uploaded SVG never reaches the decode path. The platform decoder is the
authority on the accepted bytes; there is no server-side decode.

## 3. Storage-side check

The backend does not trust the uploader. `icons::validate_icon` accepts only:

- `data:image/png` or `data:image/webp` URLs, and
- relative names (existing icons from earlier versions).

Anything with a scheme, a protocol-relative host, a backslash, or embedded
whitespace/control characters is rejected on write and withheld on read. The
CSP's `img-src 'self' data:` is the backstop for a tampered vault that bypassed
validation.

## 4. Residual

- The 8 MiB and format checks are client-side; a compromised renderer can
  still hand the backend any string, which the icon validator then rejects.
- The output is small by construction, so the vault does not grow by the input
  size, only by the 128 × 128 PNG.
