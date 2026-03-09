use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::io::{Cursor, Read};
use tar::Archive;

pub struct OspPackage {
    pub wasm_bytes: Vec<u8>,
    pub manifest_json: Vec<u8>,
    pub prompt_txt: Vec<u8>,
    pub icon_svg: Vec<u8>,
    pub signature: Option<Vec<u8>>,
}

impl OspPackage {
    /// Parse a .osp file (tar.gz) from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let cursor = Cursor::new(data);
        let gz = GzDecoder::new(cursor);
        let mut archive = Archive::new(gz);

        let mut wasm_bytes: Option<Vec<u8>> = None;
        let mut manifest_json: Option<Vec<u8>> = None;
        let mut prompt_txt: Vec<u8> = Vec::new();
        let mut icon_svg: Vec<u8> = Vec::new();
        let mut signature: Option<Vec<u8>> = None;

        let entries = archive.entries().context("failed to read tar entries")?;
        for entry in entries {
            let mut entry = entry.context("failed to read tar entry")?;
            let path = entry.path().context("failed to get entry path")?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("failed to read entry contents")?;

            match name.as_str() {
                "app.wasm" => wasm_bytes = Some(buf),
                "manifest.json" => manifest_json = Some(buf),
                "prompt.txt" => prompt_txt = buf,
                "icon.svg" => icon_svg = buf,
                "signature.sig" => signature = Some(buf),
                _ => {}
            }
        }

        let wasm_bytes = wasm_bytes.context("app.wasm not found in .osp package")?;
        let manifest_json = manifest_json.context("manifest.json not found in .osp package")?;

        Ok(OspPackage {
            wasm_bytes,
            manifest_json,
            prompt_txt,
            icon_svg,
            signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tar::Builder;

    fn make_osp_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let enc = GzEncoder::new(buf, Compression::default());
        let mut tar = Builder::new(enc);
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, name, *content).unwrap();
        }
        let enc = tar.into_inner().unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn test_parse_valid_osp() {
        let manifest = br#"{"name":"test","version":"0.1.0"}"#;
        let wasm = b"\x00asm\x01\x00\x00\x00";
        let data = make_osp_bytes(&[
            ("app.wasm", wasm),
            ("manifest.json", manifest),
            ("prompt.txt", b"build a calculator"),
            ("icon.svg", b"<svg/>"),
        ]);
        let pkg = OspPackage::from_bytes(&data).unwrap();
        assert_eq!(pkg.wasm_bytes, wasm);
        assert_eq!(pkg.manifest_json, manifest);
        assert_eq!(pkg.prompt_txt, b"build a calculator");
    }

    #[test]
    fn test_missing_wasm_fails() {
        let manifest = br#"{"name":"test","version":"0.1.0"}"#;
        let data = make_osp_bytes(&[("manifest.json", manifest)]);
        assert!(OspPackage::from_bytes(&data).is_err());
    }
}
