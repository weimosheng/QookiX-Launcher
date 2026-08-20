/** Parse the composite icon string: "bg:amber,img:C:\\path" */
export function parseIcon(value: string | null | undefined): { bg?: string; img?: string } {
  const out: { bg?: string; img?: string } = {};
  for (const part of (value ?? "").split(",")) {
    const eq = part.indexOf(":");
    if (eq < 0) continue;
    const k = part.slice(0, eq).trim();
    const v = part.slice(eq + 1).trim();
    if (!v) continue;
    if (k === "bg") out.bg = v;
    else if (k === "img") out.img = v;
  }
  return out;
}
