use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

/// Generate a new Ed25519 keypair. Returns (private_key_hex, public_key_hex).
pub fn generate_keypair() -> (String, String) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let private_hex = hex::encode(signing_key.to_bytes());
    let public_hex = hex::encode(verifying_key.to_bytes());
    (private_hex, public_hex)
}

/// Sign the content (SHA256 digest of wasm + manifest). Returns hex signature.
pub fn sign_content(
    private_key_hex: &str,
    wasm_bytes: &[u8],
    manifest_bytes: &[u8],
) -> Result<String> {
    let key_bytes = hex::decode(private_key_hex).context("invalid private key hex")?;
    let key_array: &[u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("private key must be 32 bytes"))?;
    let signing_key = SigningKey::from_bytes(key_array);
    let digest = content_digest(wasm_bytes, manifest_bytes);
    let signature: Signature = signing_key.sign(&digest);
    Ok(hex::encode(signature.to_bytes()))
}

/// Verify signature. Returns Ok(()) if valid.
pub fn verify_signature(
    public_key_hex: &str,
    sig_hex: &str,
    wasm_bytes: &[u8],
    manifest_bytes: &[u8],
) -> Result<()> {
    let key_bytes = hex::decode(public_key_hex).context("invalid public key hex")?;
    let key_array: &[u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    let verifying_key =
        VerifyingKey::from_bytes(key_array).context("invalid Ed25519 public key")?;

    let sig_bytes = hex::decode(sig_hex).context("invalid signature hex")?;
    let sig_array: &[u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let signature = Signature::from_bytes(sig_array);

    let digest = content_digest(wasm_bytes, manifest_bytes);
    verifying_key
        .verify(&digest, &signature)
        .context("signature verification failed")?;
    Ok(())
}

/// Derive the Ed25519 public key (hex) from a private key (hex).
pub fn derive_public_key(private_key_hex: &str) -> Result<String> {
    let key_bytes = hex::decode(private_key_hex).context("invalid private key hex")?;
    let key_array: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("private key must be 32 bytes"))?;
    let signing_key = SigningKey::from_bytes(&key_array);
    Ok(hex::encode(signing_key.verifying_key().to_bytes()))
}

fn content_digest(wasm_bytes: &[u8], manifest_bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(wasm_bytes);
    hasher.update(manifest_bytes);
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_verify_roundtrip() {
        let (priv_hex, pub_hex) = generate_keypair();
        let wasm = b"fake wasm bytes";
        let manifest = b"fake manifest bytes";
        let sig = sign_content(&priv_hex, wasm, manifest).unwrap();
        verify_signature(&pub_hex, &sig, wasm, manifest).unwrap();
    }

    #[test]
    fn test_verify_fails_on_tampered_content() {
        let (priv_hex, pub_hex) = generate_keypair();
        let wasm = b"fake wasm bytes";
        let manifest = b"fake manifest bytes";
        let sig = sign_content(&priv_hex, wasm, manifest).unwrap();
        // tamper with wasm
        let result = verify_signature(&pub_hex, &sig, b"tampered wasm", manifest);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_fails_on_tampered_manifest() {
        let (priv_hex, pub_hex) = generate_keypair();
        let wasm = b"wasm data";
        let manifest = b"original manifest";
        let sig = sign_content(&priv_hex, wasm, manifest).unwrap();
        let result = verify_signature(&pub_hex, &sig, wasm, b"tampered manifest");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_public_key() {
        let (priv_hex, pub_hex) = generate_keypair();
        let derived = derive_public_key(&priv_hex).unwrap();
        assert_eq!(derived, pub_hex);
    }

    #[test]
    fn test_sign_content_invalid_key_hex() {
        let result = sign_content("not-hex", b"wasm", b"manifest");
        assert!(result.is_err());
    }

    #[test]
    fn test_sign_content_wrong_key_length() {
        let result = sign_content("abcd", b"wasm", b"manifest");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_invalid_public_key_hex() {
        let result = verify_signature("not-hex", "sig", b"wasm", b"manifest");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_invalid_sig_hex() {
        let (_, pub_hex) = generate_keypair();
        let result = verify_signature(&pub_hex, "not-hex", b"wasm", b"manifest");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_wrong_sig_length() {
        let (_, pub_hex) = generate_keypair();
        let result = verify_signature(&pub_hex, "abcd", b"wasm", b"manifest");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_public_key_invalid_hex() {
        let result = derive_public_key("gggg");
        assert!(result.is_err());
    }

    #[test]
    fn test_content_digest_deterministic() {
        let d1 = content_digest(b"wasm", b"manifest");
        let d2 = content_digest(b"wasm", b"manifest");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_content_digest_changes_with_input() {
        let d1 = content_digest(b"wasm_a", b"manifest");
        let d2 = content_digest(b"wasm_b", b"manifest");
        assert_ne!(d1, d2);
    }

    #[test]
    fn test_wrong_keypair_fails_verification() {
        let (priv_hex, _pub_hex) = generate_keypair();
        let (_, other_pub) = generate_keypair();
        let wasm = b"data";
        let manifest = b"meta";
        let sig = sign_content(&priv_hex, wasm, manifest).unwrap();
        let result = verify_signature(&other_pub, &sig, wasm, manifest);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_keypair_unique() {
        let (priv1, pub1) = generate_keypair();
        let (priv2, pub2) = generate_keypair();
        assert_ne!(priv1, priv2);
        assert_ne!(pub1, pub2);
    }

    #[test]
    fn test_generate_keypair_hex_lengths() {
        let (priv_hex, pub_hex) = generate_keypair();
        // Ed25519 keys: 32 bytes = 64 hex chars
        assert_eq!(priv_hex.len(), 64);
        assert_eq!(pub_hex.len(), 64);
    }

    #[test]
    fn test_sign_content_deterministic_per_key() {
        let (priv_hex, _) = generate_keypair();
        let wasm = b"same data";
        let manifest = b"same manifest";
        let sig1 = sign_content(&priv_hex, wasm, manifest).unwrap();
        let sig2 = sign_content(&priv_hex, wasm, manifest).unwrap();
        // Ed25519 signatures are deterministic for the same key + message
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_content_digest_empty_inputs() {
        let d = content_digest(b"", b"");
        assert_eq!(d.len(), 32); // SHA256 always 32 bytes
    }
}
