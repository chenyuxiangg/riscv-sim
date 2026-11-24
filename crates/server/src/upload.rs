//! 镜像字节的两条来源（M-24 · R-04 · D2）：**上传的字节** 或 **白名单位置的
//! 路径**。
//!
//! ⚠️ **只有两条，而且都收敛到「一堆字节」**：内核收到的永远是
//! `ImageSource::Bytes` —— 路径通道是**服务端**的事，内核不知道有磁盘。
//!
//! ⚠️ **路径通道必须白名单**（`--allow-image-root`）：先规范化再判前缀，
//! 顺序不能反 —— 反了就等于「先信任客户端给的路径，再校验」（`..` 穿越在
//! 规范化之前是合法前缀）。
//!
//! ⚠️ **`.hex` / `.ihex` 在产品入口即拒**：契约的 `ImageKind` 表达不了
//! Intel HEX（只有 `Elf` / `RawHex`）⇒ ⇒ 让它进来只会在内核里变成一条
//! 「种类不支持」的怪错误。
//!
//! 本模块是**纯函数 + 一次读盘**：不碰会话、不碰 HTTP ⇒ ⇒ 可单测。

use std::path::{Path, PathBuf};

use riscv_sim_contract::model::ImageKind;
use riscv_sim_kernel::m06_elf::{detect_kind_with_hint, Kind};

use crate::error::ApiError;

/// 把请求里的路径落到白名单根下面。
///
/// - 根没配 ⇒ 400（**明确说**「路径通道没开，请直接上传字节」，而不是
///   假装文件不存在）；
/// - 解析不到（不存在 / 权限）⇒ 400（`E_PROTO-08`，这条是「请求里的路径
///   打错了」）；
/// - 规范化后**不在**根下 ⇒ 403 + `E_STORE-07`（这是**策略拒绝**，不是
///   路径写错）。
pub fn whitelist_path(root: Option<&Path>, raw: &str) -> Result<PathBuf, ApiError> {
    let Some(root) = root else {
        return Err(ApiError::bad_request(format!(
            "path source is disabled: {raw:?} needs --allow-image-root; \
             POST the image bytes instead"
        )));
    };
    if raw.trim().is_empty() {
        return Err(ApiError::bad_request("field `path` must not be blank"));
    }
    let canonical = Path::new(raw).canonicalize().map_err(|err| {
        ApiError::bad_request(format!("image path {raw:?} cannot be resolved: {err}"))
    })?;
    if !canonical.starts_with(root) {
        // ⚠️ 回**原始**路径（不是规范化后的），否则等于把服务器上的真实
        //   目录结构告诉了一个不该看它的客户端。
        return Err(ApiError::image_path_outside_root(raw));
    }
    Ok(canonical)
}

/// 取文件名里的小写扩展名（没有扩展名 ⇒ 空串）。
pub fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 判定镜像种类：**魔数优先，扩展名兜底**（与内核 M-06 同一份判据）。
///
/// ⚠️ **不另写一套判定**：调的是 `m06_elf::detect_kind_with_hint` ⇒ ⇒
/// 服务端说「这是 ELF」与内核真去解析时的判断不会分歧。
pub fn kind_for(name: &str, bytes: &[u8]) -> Result<ImageKind, ApiError> {
    let extension = extension_of(name);
    // 后缀先拒：`.hex` 是**产品口径**不支持的输入，不该等解析器判出来。
    if extension == "hex" || extension == "ihex" {
        return Err(intel_hex_rejected(name));
    }
    match detect_kind_with_hint(&extension, bytes) {
        Kind::Elf => Ok(ImageKind::Elf),
        Kind::RawHex => Ok(ImageKind::RawHex),
        // 魔数说这是 Intel HEX（首非空行是 `:`）⇒ 同样拒：契约表达不了它。
        Kind::Hex => Err(intel_hex_rejected(name)),
    }
}

/// Intel HEX 的拒绝口径（两处入口**共用一句**话，免得文案漂）。
fn intel_hex_rejected(name: &str) -> ApiError {
    ApiError::bad_request(format!(
        "image {name:?} is Intel HEX; the product entry accepts ELF or a raw .bin only"
    ))
}

/// 从路径里取「镜像名」（响应用它回显，客户端据此对上自己那份）。
pub fn name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| "image.bin".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_is_lowercased_and_empty_without_a_dot() {
        assert_eq!(extension_of("Firmware.ELF"), "elf");
        assert_eq!(extension_of("fw.bin"), "bin");
        assert_eq!(extension_of("noext"), "");
    }

    #[test]
    fn an_elf_magic_wins_over_a_misleading_extension() {
        // ELF32 头的头四个字节就够判了。
        let mut bytes = vec![0x7F, b'E', b'L', b'F'];
        bytes.extend_from_slice(&[0; 16]);
        assert_eq!(kind_for("firmware.bin", &bytes).unwrap(), ImageKind::Elf);
    }

    #[test]
    fn a_plain_blob_is_raw_hex() {
        assert_eq!(
            kind_for("firmware.bin", &[0x13, 0x00, 0x00, 0x00]).unwrap(),
            ImageKind::RawHex
        );
    }

    #[test]
    fn intel_hex_is_rejected_at_the_product_entry() {
        // 后缀先拒。
        let err = kind_for("firmware.hex", b"\x7FELF").unwrap_err();
        assert_eq!(err.code(), "E_PROTO-08");
        assert!(err.detail().contains("Intel HEX"));
        // 没有后缀、但内容是**一条合法 Intel HEX 记录**也拒（魔数优先）。
        // ⚠️ 这里必须用**合法记录**：M-06 的判据是「形状像记录」（长度、
        //   十六进制、校验和），不是「以冒号开头」。
        let err = kind_for(
            "firmware.dat",
            b":10010000214601360121470136007EFE09D2190140\n",
        )
        .unwrap_err();
        assert_eq!(err.code(), "E_PROTO-08");
    }

    #[test]
    fn a_path_without_a_whitelist_root_is_a_bad_request_not_a_missing_file() {
        let err = whitelist_path(None, "/tmp/fw.elf").unwrap_err();
        assert_eq!(err.code(), "E_PROTO-08");
        assert!(err.detail().contains("--allow-image-root"));
    }

    #[test]
    fn a_path_outside_the_root_is_forbidden() {
        let root = std::env::temp_dir();
        let outside = std::path::Path::new("/etc/hosts");
        if !outside.exists() {
            return;
        }
        let err = whitelist_path(Some(&root), "/etc/hosts").unwrap_err();
        // ⚠️ `/etc/hosts` 是通过符号链接指到 `/private/etc/hosts`（macOS）
        //    ⇒ 规范化之后仍在 root 之外 ⇒ 403 成立。
        assert_eq!(err.code(), "E_STORE-07");
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn a_path_inside_the_root_resolves() {
        let dir = std::env::temp_dir();
        let file = dir.join("m24_whitelist_probe.bin");
        std::fs::write(&file, b"\x13\x00\x00\x00").unwrap();
        let canonical_dir = dir.canonicalize().unwrap();
        let resolved = whitelist_path(Some(&canonical_dir), file.to_str().unwrap()).unwrap();
        assert!(resolved.starts_with(&canonical_dir));
        assert_eq!(name_from_path(&resolved), "m24_whitelist_probe.bin");
        std::fs::remove_file(&file).ok();
    }
}
