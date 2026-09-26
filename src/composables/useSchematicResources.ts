import { CanvasTexture, NearestFilter, LinearMipmapLinearFilter, SRGBColorSpace } from "three";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";

export interface BlockState {
  name: string;
  properties: Record<string, string>;
}

export interface SchematicResources {
  texture: CanvasTexture;
  atlasUrl: string;
  atlasWidth: number;
  atlasHeight: number;
  resourceBase: string;
  getBlockIconRect: (blockName: string) => [number, number, number, number] | null;
}

type ModelEntry = { parent?: string; textures?: Record<string, string> };
type TextureLayout = Record<string, [number, number, number, number]>;

const ICON_TEXTURE_PRIORITY = ["all", "side", "top", "texture", "bottom", "end", "north", "particle"];

function resolveIconTexture(
  modelKey: string,
  modelIndex: Record<string, ModelEntry>,
  seen: Set<string>,
): string | null {
  if (seen.has(modelKey)) return null;
  seen.add(modelKey);
  const model = modelIndex[modelKey];
  if (!model) return null;
  if (model.textures) {
    for (const key of ICON_TEXTURE_PRIORITY) {
      if (model.textures[key]) return model.textures[key];
    }
    const vals = Object.values(model.textures);
    if (vals.length) return vals[0];
  }
  if (model.parent) {
    const parentKey = model.parent.replace("minecraft:", "");
    return resolveIconTexture(parentKey, modelIndex, seen);
  }
  return null;
}

async function resolveResourceBase(): Promise<string> {
  const cachePath = await invoke<string>("schematic_extract_resources");
  return convertFileSrc(cachePath);
}

export async function loadSchematicResources(): Promise<SchematicResources> {
  const resourceBase = await resolveResourceBase();
  console.log("[schematic] resourceBase:", resourceBase);
  const atlasUrl = `${resourceBase}/texture-atlas.png`;
  const [res, modelRes, layoutRes] = await Promise.all([
    fetch(atlasUrl),
    fetch(`${resourceBase}/block-model-index.json`),
    fetch(`${resourceBase}/texture-layout.json`),
  ]);
  console.log("[schematic] fetch status:", { atlas: res.status, model: modelRes.status, layout: layoutRes.status });
  if (!res.ok) throw new Error("纹理图集加载失败：请先安装 Minecraft 客户端");
  const blob = await res.blob();
  const bitmap = await createImageBitmap(blob);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bitmap, 0, 0);
  bitmap.close();

  const texture = new CanvasTexture(canvas);
  texture.flipY = true;
  texture.magFilter = NearestFilter;
  texture.minFilter = LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.colorSpace = SRGBColorSpace;
  texture.needsUpdate = true;

  const modelIndex = modelRes.ok ? await modelRes.json() as Record<string, ModelEntry> : {};
  const textureLayout = layoutRes.ok ? await layoutRes.json() as TextureLayout : {};
  const atlasWidth = canvas.width;
  const atlasHeight = canvas.height;
  console.log("[schematic] atlas:", atlasWidth, "x", atlasHeight, "| models:", Object.keys(modelIndex).length, "| textures:", Object.keys(textureLayout).length);
  console.log("[schematic] sample texture keys:", Object.keys(textureLayout).slice(0, 5));

  try {
    const dbgRes = await fetch(`${resourceBase}/_debug.json`);
    if (dbgRes.ok) console.log("[schematic] debug:", await dbgRes.json());
  } catch { /* ignore */ }

  function getBlockIconRect(blockName: string): [number, number, number, number] | null {
    const shortName = blockName.replace("minecraft:", "");
    const modelKey = shortName.startsWith("block/") ? shortName : `block/${shortName}`;
    const texName = resolveIconTexture(modelKey, modelIndex, new Set());
    if (!texName) return null;
    const layoutKey = texName.replace("minecraft:", "");
    const rect = textureLayout[layoutKey];
    if (!rect) return null;
    return rect;
  }

  return { texture, atlasUrl, atlasWidth, atlasHeight, resourceBase, getBlockIconRect };
}
