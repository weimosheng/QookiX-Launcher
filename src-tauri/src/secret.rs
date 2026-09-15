//! 敏感令牌（GitHub OAuth token 等）落盘保护。
//!
//! Windows 上使用 DPAPI（`CryptProtectData`）：密钥由系统按当前用户派生，
//! 文件被拷到其他机器或其他用户下无法解密，无需应用侧管理密钥。
//! 非 Windows 平台退回简单混淆（与既有账号文件做法一致）。

use base64::Engine as _;

/// 加密明文，返回可直接写入 JSON 的字符串（Windows: base64(DPAPI blob)）。
pub fn protect(plain: &str) -> Result<String, String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::CryptProtectData;
        use windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB;

        let data = plain.as_bytes();
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // CRYPTPROTECT_UI_FORBIDDEN：后台调用，不允许弹窗
        let ok = unsafe {
            CryptProtectData(
                &mut input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1, // CRYPTPROTECT_UI_FORBIDDEN
                &mut output,
            )
        };
        if ok == 0 {
            return Err("DPAPI 加密失败".into());
        }
        let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
        let encoded = base64::engine::general_purpose::STANDARD.encode(slice);
        unsafe {
            LocalFree(output.pbData as _);
        }
        Ok(encoded)
    }
    #[cfg(not(windows))]
    {
        Ok(crate::cloud_sync::store::obfuscate(plain))
    }
}

/// 解密 protect() 产生的内容。
pub fn unprotect(encoded: &str) -> Result<String, String> {
    if encoded.is_empty() {
        return Ok(String::new());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::CryptUnprotectData;
        use windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB;

        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("密文 base64 解码失败: {e}"))?;
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &mut input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1, // CRYPTPROTECT_UI_FORBIDDEN
                &mut output,
            )
        };
        if ok == 0 {
            return Err("DPAPI 解密失败（可能来自其他用户/机器）".into());
        }
        let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
        let plain = String::from_utf8_lossy(slice).to_string();
        unsafe {
            LocalFree(output.pbData as _);
        }
        Ok(plain)
    }
    #[cfg(not(windows))]
    {
        Ok(crate::cloud_sync::store::deobfuscate(encoded))
    }
}
