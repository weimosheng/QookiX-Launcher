import * as deepslate from "deepslate";

const CHUNK_SIZE = 16;

const AIR_NAMES = new Set([
  "minecraft:air",
  "minecraft:cave_air",
  "minecraft:void_air",
  "minecraft:light",
  "minecraft:barrier",
  "minecraft:structure_void",
]);

const TRANSLUCENT_PARTS = [
  "glass", "ice", "water", "lava", "slime_block", "honey_block",
  "nether_portal", "end_gateway",
];

export interface BlockState {
  name: string;
  properties: Record<string, string>;
}

export interface MeshData {
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  colors: Float32Array;
}

export interface MeshResult {
  opaque: MeshData;
  translucent: MeshData;
}

let resourceBase = "";

function identifier(value: string): string {
  const n = value.trim().toLowerCase();
  return n.includes(":") ? n : `minecraft:${n}`;
}

function isAir(name: string): boolean {
  return AIR_NAMES.has(name);
}

function isTranslucent(name: string): boolean {
  return TRANSLUCENT_PARTS.some((p) => name.includes(p));
}

function isOccluding(state: BlockState | undefined): boolean {
  if (!state || isAir(state.name)) return false;
  if (isTranslucent(state.name)) return false;
  return fullCubeBlocks.has(state.name);
}

interface MeshBuffers {
  positions: number[];
  normals: number[];
  uvs: number[];
  colors: number[];
}

function emptyBuffers(): MeshBuffers {
  return { positions: [], normals: [], uvs: [], colors: [] };
}

function appendQuad(
  buf: MeshBuffers,
  verts: Array<{ pos: { x: number; y: number; z: number }; texture?: [number, number]; color: [number, number, number] }>,
  ox: number, oy: number, oz: number,
) {
  const e1x = verts[1].pos.x - verts[0].pos.x;
  const e1y = verts[1].pos.y - verts[0].pos.y;
  const e1z = verts[1].pos.z - verts[0].pos.z;
  const e2x = verts[2].pos.x - verts[0].pos.x;
  const e2y = verts[2].pos.y - verts[0].pos.y;
  const e2z = verts[2].pos.z - verts[0].pos.z;
  let nx = e1y * e2z - e1z * e2y;
  let ny = e1z * e2x - e1x * e2z;
  let nz = e1x * e2y - e1y * e2x;
  const len = Math.hypot(nx, ny, nz) || 1;
  nx /= len; ny /= len; nz /= len;
  for (const i of [0, 1, 2, 0, 2, 3]) {
    const v = verts[i];
    buf.positions.push(v.pos.x + ox, v.pos.y + oy, v.pos.z + oz);
    buf.normals.push(nx, ny, nz);
    buf.uvs.push(v.texture?.[0] ?? 0, v.texture?.[1] ?? 0);
    buf.colors.push(v.color[0], v.color[1], v.color[2]);
  }
}

function appendFallbackCube(buf: MeshBuffers, name: string, missUv: [number, number, number, number]) {
  const [u0, v0, u1, v1] = missUv;
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) | 0;
  const color: [number, number, number] = [
    0.55 + ((h >>> 0) & 0xff) / 640,
    0.55 + ((h >>> 8) & 0xff) / 640,
    0.55 + ((h >>> 16) & 0xff) / 640,
  ];
  const faces: Array<Array<[number, number, number, number, number]>> = [
    [[0,0,0,u0,v0],[1,0,0,u1,v0],[1,0,1,u1,v1],[0,0,1,u0,v1]],
    [[0,1,1,u0,v1],[1,1,1,u1,v1],[1,1,0,u1,v0],[0,1,0,u0,v0]],
    [[0,0,1,u0,v1],[1,0,1,u1,v1],[1,1,1,u1,v0],[0,1,1,u0,v0]],
    [[1,0,0,u0,v1],[0,0,0,u1,v1],[0,1,0,u1,v0],[1,1,0,u0,v0]],
    [[1,0,1,u0,v1],[1,0,0,u1,v1],[1,1,0,u1,v0],[1,1,1,u0,v0]],
    [[0,0,0,u0,v1],[0,0,1,u1,v1],[0,1,1,u1,v0],[0,1,0,u0,v0]],
  ];
  for (const face of faces) {
    appendQuad(buf, face.map(([px, py, pz, u, v]) => ({
      pos: { x: px, y: py, z: pz },
      texture: [u, v] as [number, number],
      color,
    })), 0, 0, 0);
  }
}

let initialized = false;
let blockDefinitions: Record<string, deepslate.BlockDefinition> = {};
let blockModels: Record<string, deepslate.BlockModel> = {};
let textureUvs: Record<string, [number, number, number, number]> = {};
let defaultBlockProperties: Record<string, Record<string, string>> = {};
let missingTextureUv: [number, number, number, number] = [0, 0, 1, 1];
let atlasProvider: deepslate.TextureAtlasProvider;
let modelProvider: deepslate.BlockModelProvider;
let fullCubeBlocks = new Set<string>();

function computeFullCubeBlocks() {
  const noCull: deepslate.Cull = {
    up: false, down: false, north: false, south: false, east: false, west: false,
  };
  const EPS = 0.1;
  for (const name of Object.keys(blockDefinitions)) {
    const props = defaultBlockProperties[name] ?? {};
    const mesh = new deepslate.Mesh();
    try {
      const blockState = new deepslate.BlockState(name, props);
      mesh.merge(deepslate.SpecialRenderers.getBlockMesh(blockState, undefined, atlasProvider, noCull));
    } catch { /* skip special */ }
    try {
      mesh.merge(blockDefinitions[name].getMesh(deepslate.Identifier.parse(name), props, atlasProvider, modelProvider, noCull));
    } catch { /* skip def */ }
    if (mesh.quads.length === 0) continue;
    let up = false, down = false, north = false, south = false, east = false, west = false;
    for (const quad of mesh.quads) {
      const v = quad.vertices();
      const xs = [v[0].pos.x, v[1].pos.x, v[2].pos.x, v[3].pos.x];
      const ys = [v[0].pos.y, v[1].pos.y, v[2].pos.y, v[3].pos.y];
      const zs = [v[0].pos.z, v[1].pos.z, v[2].pos.z, v[3].pos.z];
      const minX = Math.min(...xs), maxX = Math.max(...xs);
      const minY = Math.min(...ys), maxY = Math.max(...ys);
      const minZ = Math.min(...zs), maxZ = Math.max(...zs);
      if (minY > 16 - EPS && maxY < 16 + EPS && minX < EPS && maxX > 16 - EPS && minZ < EPS && maxZ > 16 - EPS) up = true;
      if (minY > -EPS && maxY < EPS && minX < EPS && maxX > 16 - EPS && minZ < EPS && maxZ > 16 - EPS) down = true;
      if (minZ > -EPS && maxZ < EPS && minX < EPS && maxX > 16 - EPS && minY < EPS && maxY > 16 - EPS) north = true;
      if (minZ > 16 - EPS && maxZ < 16 + EPS && minX < EPS && maxX > 16 - EPS && minY < EPS && maxY > 16 - EPS) south = true;
      if (minX > 16 - EPS && maxX < 16 + EPS && minY < EPS && maxY > 16 - EPS && minZ < EPS && maxZ > 16 - EPS) east = true;
      if (minX > -EPS && maxX < EPS && minY < EPS && maxY > 16 - EPS && minZ < EPS && maxZ > 16 - EPS) west = true;
    }
    if (up && down && north && south && east && west) fullCubeBlocks.add(name);
  }
}

async function initResources() {
  async function fetchJson(url: string): Promise<unknown> {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`Worker 资源加载失败: ${url}`);
    return res.json();
  }

  const [layoutRes, stateRes, modelRes, defaultsRes] = await Promise.all([
    fetchJson(`${resourceBase}/texture-layout.json`),
    fetchJson(`${resourceBase}/block-state-index.json`),
    fetchJson(`${resourceBase}/block-model-index.json`),
    fetchJson(`${resourceBase}/block-property-defaults.json`),
  ]);

  const atlasRes = await fetch(`${resourceBase}/texture-atlas.png`);
  if (!atlasRes.ok) throw new Error("Worker: 纹理图集加载失败");
  const atlasBlob = await atlasRes.blob();
  const atlasBitmap = await createImageBitmap(atlasBlob);
  const atlasW = atlasBitmap.width;
  const atlasH = atlasBitmap.height;
  atlasBitmap.close();

  const rawUvs = layoutRes as Record<string, [number, number, number, number]>;
  console.log("[worker] resourceBase:", resourceBase, "| layout entries:", Object.keys(rawUvs).length, "| atlas:", atlasW, "x", atlasH);
  for (const [key, [px, py, pw, ph]] of Object.entries(rawUvs)) {
    const nk = identifier(key);
    textureUvs[nk] = [px / atlasW, 1 - (py + ph) / atlasH, (px + pw) / atlasW, 1 - py / atlasH];
  }
  console.log("[worker] textureUvs entries:", Object.keys(textureUvs).length, "| sample:", Object.keys(textureUvs).slice(0, 3));
  missingTextureUv = textureUvs["minecraft:block/gray_concrete"] ?? [0, 0, 1 / 64, 1 / 64];

  const rawDefaults = defaultsRes as Record<string, Record<string, string>>;
  for (const [key, value] of Object.entries(rawDefaults)) {
    defaultBlockProperties[identifier(key)] = value;
  }

  const rawDefs = stateRes as Record<string, unknown>;
  for (const [key, value] of Object.entries(rawDefs)) {
    try { blockDefinitions[identifier(key)] = deepslate.BlockDefinition.fromJson(value); } catch { /* skip */ }
  }

  const rawModels = modelRes as Record<string, unknown>;
  for (const [key, value] of Object.entries(rawModels)) {
    try { blockModels[identifier(key)] = deepslate.BlockModel.fromJson(value); } catch { /* skip */ }
  }

  modelProvider = {
    getBlockModel(id: deepslate.Identifier) { return blockModels[id.toString()] ?? null; },
  };
  for (const [id, model] of Object.entries(blockModels)) {
    try { model.flatten(modelProvider); } catch { delete blockModels[id]; }
  }

  atlasProvider = {
    getTextureAtlas() { return new ImageData(1, 1); },
    getTextureUV(id: deepslate.Identifier) { return textureUvs[id.toString()] ?? missingTextureUv; },
  };

  computeFullCubeBlocks();
  initialized = true;
}

function buildBlockMesh(state: BlockState, cull: deepslate.Cull): deepslate.Mesh {
  const props = { ...(defaultBlockProperties[state.name] ?? {}), ...state.properties };
  const blockState = new deepslate.BlockState(state.name, props);
  let mesh: deepslate.Mesh;
  try {
    mesh = deepslate.SpecialRenderers.getBlockMesh(blockState, undefined, atlasProvider, cull);
  } catch {
    mesh = new deepslate.Mesh();
  }
  const def = blockDefinitions[state.name];
  if (def) {
    try {
      mesh.merge(def.getMesh(deepslate.Identifier.parse(state.name), props, atlasProvider, modelProvider, cull));
    } catch { /* skip */ }
  }
  return mesh;
}

function toMeshData(buf: MeshBuffers): MeshData {
  return {
    positions: new Float32Array(buf.positions),
    normals: new Float32Array(buf.normals),
    uvs: new Float32Array(buf.uvs),
    colors: new Float32Array(buf.colors),
  };
}

function buildChunkMesh(chunkPos: [number, number, number], blocks: Uint32Array, palette: BlockState[]): MeshResult {
  const opaque = emptyBuffers();
  const translucent = emptyBuffers();
  const [cx, cy, cz] = chunkPos;
  const ox = cx * CHUNK_SIZE, oy = cy * CHUNK_SIZE, oz = cz * CHUNK_SIZE;

  for (let y = 0; y < CHUNK_SIZE; y++) {
    for (let z = 0; z < CHUNK_SIZE; z++) {
      for (let x = 0; x < CHUNK_SIZE; x++) {
        const pi = blocks[y * 256 + z * 16 + x];
        if (pi === 0) continue;
        const state = palette[pi];
        if (!state || isAir(state.name)) continue;

        const cull: deepslate.Cull = {
          west: isOccluding(x > 0 ? palette[blocks[y * 256 + z * 16 + (x - 1)]] : undefined),
          east: isOccluding(x < 15 ? palette[blocks[y * 256 + z * 16 + (x + 1)]] : undefined),
          down: isOccluding(y > 0 ? palette[blocks[(y - 1) * 256 + z * 16 + x]] : undefined),
          up: isOccluding(y < 15 ? palette[blocks[(y + 1) * 256 + z * 16 + x]] : undefined),
          north: isOccluding(z > 0 ? palette[blocks[y * 256 + (z - 1) * 16 + x]] : undefined),
          south: isOccluding(z < 15 ? palette[blocks[y * 256 + (z + 1) * 16 + x]] : undefined),
        };

        const mesh = buildBlockMesh(state, cull);
        const target = isTranslucent(state.name) ? translucent : opaque;

        if (mesh.quads.length === 0) {
          appendFallbackCube(target, state.name, missingTextureUv);
          continue;
        }
        for (const quad of mesh.quads) {
          const verts = quad.vertices().map((item) => ({
            pos: { x: item.pos.x, y: item.pos.y, z: item.pos.z },
            texture: item.texture ? [item.texture[0], item.texture[1]] as [number, number] : undefined,
            color: item.color as [number, number, number],
          }));
          appendQuad(target, verts, ox + x, oy + y, oz + z);
        }
      }
    }
  }
  return { opaque: toMeshData(opaque), translucent: toMeshData(translucent) };
}

self.onmessage = async (event: MessageEvent) => {
  const msg = event.data;
  if (msg.type === "init") {
    if (msg.resourceBase) resourceBase = msg.resourceBase;
    await initResources();
    (self as unknown as Worker).postMessage({ type: "ready" });
  } else if (msg.type === "mesh") {
    if (!initialized) throw new Error("Worker not initialized");
    const result = buildChunkMesh(msg.chunkPos, msg.blocks, msg.palette);
    (self as unknown as Worker).postMessage({
      type: "meshResult",
      chunkPos: msg.chunkPos,
      opaque: result.opaque,
      translucent: result.translucent,
    }, [
      result.opaque.positions.buffer, result.opaque.normals.buffer,
      result.opaque.uvs.buffer, result.opaque.colors.buffer,
      result.translucent.positions.buffer, result.translucent.normals.buffer,
      result.translucent.uvs.buffer, result.translucent.colors.buffer,
    ]);
  }
};
