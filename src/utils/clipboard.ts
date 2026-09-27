/**
 * 剪贴板工具。
 *
 * 此前各处的复制都是各自写一遍 try/catch + textarea 兜底（FileManager、
 * ServerFileManager、LogViewer…）。新代码统一走这里，旧调用点可逐步迁移。
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // 兜底：临时 textarea + execCommand（部分 WebView / 非安全上下文下 clipboard API 不可用）
    try {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(ta);
      return ok;
    } catch {
      return false;
    }
  }
}
