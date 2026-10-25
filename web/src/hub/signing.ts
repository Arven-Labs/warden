// Signing a skill package in the browser (P160), with the key file `warden-server package keygen` (or the desktop app) makes. The hub
// exports the package without a signature and the signature is made here, so the private key never leaves this page: it is read into
// memory for the one signature and dropped. The scheme is the one `warden-core`'s `signing.rs` checks: Ed25519 over the bytes of the
// package's `hash`, written `ed25519:<public key>:<signature>` in the package's `signature` field. Kept out of the component so it is
// testable without a DOM.

/** Something wrong with the key file or with the browser's ability to sign, in words for the person. */
export class SigningError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "SigningError";
  }
}

// The key file holds the private key as the PKCS#8 that `ring` writes, in hex: 16 bytes of header, the 32-byte seed, 5 bytes that
// introduce the public key, and the 32-byte public key (85 in all). WebCrypto reads the older v1 shape, which is the same header
// (with the version and length of that shape) and the seed alone.
const V2_HEADER = "3053020101300506032b657004220420";
const V1_HEADER = "302e020100300506032b657004220420";
const PUBLIC_KEY_INTRO = "a123032100";
const SEED_START = V2_HEADER.length / 2;
const PUBLIC_INTRO_START = SEED_START + 32;
const PUBLIC_START = PUBLIC_INTRO_START + PUBLIC_KEY_INTRO.length / 2;
const KEY_FILE_BYTES = PUBLIC_START + 32;

const NOT_A_KEY = "This is not a Warden signing key file.";

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function fromHex(text: string): Uint8Array | null {
  if (text.length === 0 || text.length % 2 !== 0 || !/^[0-9a-fA-F]+$/.test(text)) return null;
  return Uint8Array.from(text.match(/../g) ?? [], (pair) => parseInt(pair, 16));
}

/** The seed and the public key inside a key file's text, or `null` when it is not a Warden signing key. */
export function readKeyFile(text: string): { seed: Uint8Array; publicKey: Uint8Array } | null {
  if (text.length > 4096) return null;
  const bytes = fromHex(text.trim());
  if (bytes === null || bytes.length !== KEY_FILE_BYTES) return null;
  if (toHex(bytes.slice(0, SEED_START)) !== V2_HEADER || toHex(bytes.slice(PUBLIC_INTRO_START, PUBLIC_START)) !== PUBLIC_KEY_INTRO) return null;
  return { seed: bytes.slice(SEED_START, PUBLIC_INTRO_START), publicKey: bytes.slice(PUBLIC_START) };
}

function asBuffer(bytes: Uint8Array): BufferSource {
  return bytes as unknown as BufferSource;
}

/** The public key (64 hex characters) of a key file's text, without signing anything. */
export function publicKeyOf(keyFileText: string): string {
  const key = readKeyFile(keyFileText);
  if (key === null) throw new SigningError(NOT_A_KEY);
  return toHex(key.publicKey);
}

/** `content` (a package, as the hub exported it) with the signature of the key file's owner, and the key that signed it. The result is
 * checked here against the public key in the file; the caller should still have the hub read it back, which is the check that
 * the hub accepts it. */
export async function signPackageText(content: string, keyFileText: string): Promise<{ content: string; signer: string }> {
  const key = readKeyFile(keyFileText);
  if (key === null) throw new SigningError(NOT_A_KEY);
  let envelope: Record<string, unknown>;
  try {
    envelope = JSON.parse(content) as Record<string, unknown>;
  } catch {
    throw new SigningError("This is not a Warden package.");
  }
  const hash = envelope.hash;
  if (envelope.format !== "warden-package" || typeof hash !== "string" || !hash.startsWith("sha256:")) throw new SigningError("This is not a Warden package.");

  const subtle = globalThis.crypto?.subtle;
  if (!subtle) throw new SigningError("This browser cannot sign here: it has no WebCrypto.");
  const message = new TextEncoder().encode(hash);
  let signature: Uint8Array;
  try {
    const pkcs8 = Uint8Array.from([...(fromHex(V1_HEADER) as Uint8Array), ...key.seed]);
    const privateKey = await subtle.importKey("pkcs8", asBuffer(pkcs8), { name: "Ed25519" }, false, ["sign"]);
    signature = new Uint8Array(await subtle.sign({ name: "Ed25519" }, privateKey, asBuffer(message)));
    // The public key in the file must be the seed's: a file where they differ would make a signature nobody can check.
    const publicKey = await subtle.importKey("raw", asBuffer(key.publicKey), { name: "Ed25519" }, false, ["verify"]);
    if (!(await subtle.verify({ name: "Ed25519" }, publicKey, asBuffer(signature), asBuffer(message)))) throw new SigningError(NOT_A_KEY);
  } catch (err) {
    if (err instanceof SigningError) throw err;
    throw new SigningError("This browser cannot sign with this kind of key (Ed25519). Sign with the desktop app or `warden-server package sign`.");
  }

  const signer = toHex(key.publicKey);
  envelope.signature = `ed25519:${signer}:${toHex(signature)}`;
  return { content: JSON.stringify(envelope, null, 2), signer };
}
