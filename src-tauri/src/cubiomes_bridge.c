// C bridge: simplify Rust FFI so Rust doesn't need to declare cubiomes'
// large Generator union. setupGenerator/applySeed do not malloc internally,
// so malloc/free here leaks nothing.
#include <math.h>
#include <string.h>
#include "generator.h"
#include "finders.h"

// 山体阴影参数（与前端一致：光从西北，约 45° 高度角）
#define SHADE_EXAG      7.0
#define SHADE_AZ        3.9269908169872414  // 225°
#define SHADE_ALT       0.7853981633974483  // 45°
#define SHADE_FLAT      0.7071067811865476  // sin(45°)
#define SHADE_MIN       0.5
#define SHADE_MAX       1.4
#define SHADE_UNIT      62.0
#define SHADE_PER_DEPTH 32.0

// Query biome id at (x,y,z). scale=1 for block coords, 4 for biome coords.
// flags: LARGE_BIOMES / FORCE_OCEAN_VARIANTS (see generator.h).
// Returns biome id, or -1 for none/failure.
int bridge_query_biome(uint64_t seed, int mc, int dim, int scale,
                       int x, int y, int z, uint32_t flags) {
    Generator *g = (Generator *)malloc(sizeof(Generator));
    if (!g) return -1;
    setupGenerator(g, mc, flags);
    applySeed(g, dim, seed);
    int b = getBiomeAt(g, scale, x, y, z);
    free(g);
    return b;
}

// Biome elevation estimate: depth/scale from cubed biome parameters.
// Returns nonzero on success.
int bridge_biome_depth(int id, float *depth, float *scale) {
    double d = 0, s = 0;
    getBiomeDepthAndScale(id, &d, &s, 0);
    if (depth) *depth = (float)d;
    if (scale) *scale = (float)s;
    return 1;
}

// 估算某列的地表高度（方块），给 /tp 指令用。
//
// 1.18+ 主世界：生物群系深度参数在 y=0 处为 d = 1 - 高度/32 - 0.519 + off，
// 而地表就是 d=0 的地方，于是地表高度 ≈ 128 * d（y=0 处的 d 由噪声算出，
// 与山体阴影用的是同一个值）。
// cubiomes 自己也拿这个参数当高度用（finders.c 的 isViableStructureTerrain：
// “depth parameter (0.5 ~ sea level)”，恰好对应这里的 128*0.5 = 64）。
// 估算有几格误差，所以再 +3：宁可略高一点、落地后正常往下走，也别卡在石头里。
// 旧版本 / 下界 / 末地：退化成群系基准高度。
int bridge_surface_height(uint64_t seed, int mc, int dim, int x, int z) {
    Generator *g = (Generator *)malloc(sizeof(Generator));
    if (!g) return 0;
    setupGenerator(g, mc, 0);
    applySeed(g, dim, seed);
    int h;
    if (mc >= MC_1_18 && dim == DIM_OVERWORLD && g->bn.nptype == -1) {
        int64_t np[NP_MAX];
        sampleBiomeNoise(&g->bn, np, floordiv(x, 4), 0, floordiv(z, 4), 0, 0);
        h = (int)lround((double)np[NP_DEPTH] * 128.0 / 10000.0) + 3;
    } else {
        int id = getBiomeAt(g, 4, floordiv(x, 4), 0, floordiv(z, 4));
        double d = 0, s = 0;
        getBiomeDepthAndScale(id, &d, &s, 0);
        h = (int)lround(SHADE_UNIT + d * SHADE_PER_DEPTH) + 3;
    }
    free(g);
    return h;
}

// Structure generation attempt position in region (regX, regZ).
// Returns nonzero if valid; writes block pos to *px/*pz.
int bridge_get_structure_pos(int stype, uint64_t seed, int mc, int regX, int regZ, int *px, int *pz) {
    Pos pos = {0, 0};
    int ok = getStructurePos(stype, mc, seed, regX, regZ, &pos);
    if (ok) {
        if (px) *px = pos.x;
        if (pz) *pz = pos.z;
    }
    return ok;
}

// Structure config: regionSize (in chunks), chunkRange.
// Returns nonzero if the version supports the structure type.
int bridge_get_structure_info(int stype, int mc, int *regionSize, int *chunkRange) {
    StructureConfig sc;
    if (!getStructureConfig(stype, mc, &sc)) return 0;
    if (regionSize) *regionSize = sc.regionSize;
    if (chunkRange) *chunkRange = sc.chunkRange;
    return 1;
}

// 世界出生点（cubiomes getSpawn：会做若干次群系/高度采样，比 estimateSpawn 准）。
// 返回 0 表示失败。
int bridge_get_spawn(uint64_t seed, int mc, uint32_t flags, int *px, int *pz) {
    Generator *g = (Generator *)malloc(sizeof(Generator));
    if (!g) return 0;
    setupGenerator(g, mc, flags);
    applySeed(g, DIM_OVERWORLD, seed);
    Pos p = getSpawn(g);
    free(g);
    if (px) *px = p.x;
    if (pz) *pz = p.z;
    return 1;
}

// Is slime chunk?
int bridge_is_slime_chunk(uint64_t seed, int chunkX, int chunkZ) {
    return isSlimeChunk(seed, chunkX, chunkZ);
}

// 3x3 均值平滑（用于群系基准高度估算出的高程，避免群系边界出现硬棱）
static void blur_field(float *f, int g) {
    float *tmp = (float *)malloc(sizeof(float) * g * g);
    if (!tmp) return;
    for (int y = 0; y < g; y++) {
        for (int x = 0; x < g; x++) {
            float sum = 0;
            int n = 0;
            for (int dy = -1; dy <= 1; dy++) {
                int yy = y + dy;
                if (yy < 0 || yy >= g) continue;
                for (int dx = -1; dx <= 1; dx++) {
                    int xx = x + dx;
                    if (xx < 0 || xx >= g) continue;
                    sum += f[yy * g + xx];
                    n++;
                }
            }
            tmp[y * g + x] = sum / n;
        }
    }
    memcpy(f, tmp, sizeof(float) * g * g);
    free(tmp);
}

// 由高程网格 hs*hs（含 1 圈 padding）计算山体阴影，输出 (hs-2)^2 个字节。
// 字节值 v 对应亮度系数 f = SHADE_MIN + v/255 * (SHADE_MAX - SHADE_MIN)。
static void compute_shade(const float *h, int hs, int stepBlocks, uint8_t *out) {
    int gs = hs - 2;
    double lx = cos(SHADE_ALT) * sin(SHADE_AZ);
    double ly = cos(SHADE_ALT) * cos(SHADE_AZ);
    double lz = sin(SHADE_ALT);
    double denom = (2.0 * stepBlocks) / SHADE_EXAG;
    double scale = 255.0 / (SHADE_MAX - SHADE_MIN);
    for (int gy = 0; gy < gs; gy++) {
        for (int gx = 0; gx < gs; gx++) {
            int c = (gy + 1) * hs + (gx + 1);
            // tanh 软压缩：陡坡不会出现死黑 / 死白
            double nx = -3.0 * tanh((h[c + 1] - h[c - 1]) / denom / 3.0);
            double ny = -3.0 * tanh((h[c + hs] - h[c - hs]) / denom / 3.0);
            double len = sqrt(nx * nx + ny * ny + 1.0);
            double s = (nx * lx + ny * ly + lz) / len;
            double f = 1.0 + (s - SHADE_FLAT) * 1.1;
            if (f < SHADE_MIN) f = SHADE_MIN;
            else if (f > SHADE_MAX) f = SHADE_MAX;
            int v = (int)((f - SHADE_MIN) * scale + 0.5);
            if (v < 0) v = 0;
            else if (v > 255) v = 255;
            out[gy * gs + gx] = (uint8_t)v;
        }
    }
}

// 生成 sx*sz 的生物群系网格（每格 1 字节，负值写 255）。
// 若 out_shade 非空，同时输出 (hs-2)^2 个山体阴影亮度字节：
//   高程采样在固定的 1:4（4 方块）世界栅格上，起点为 1:4 坐标 (hx4, hz4)，
//   步长 hstep4（1:4 单位）。因为该栅格与显示缩放无关，任何缩放级别下地貌
//   形状一致，只是采样更稀疏。
//   kstep = 每个高程样本跨越的生物群系单元格数（= 生物格步长 × 阴影降采样倍数）。
// 1.18+ 主世界用 cubiomes 的真实深度噪声当高程；旧版本退化为群系基准高度。
// 返回 0 表示成功。
int bridge_gen_biome_map(uint64_t seed, int mc, int dim, int scale,
                         int x, int y, int z, int sx, int sz, uint32_t flags,
                         uint8_t *out, uint8_t *out_shade, int hs, int hstep4, int hx4, int hz4, int kstep) {
    Generator *g = (Generator *)malloc(sizeof(Generator));
    if (!g) return -1;
    setupGenerator(g, mc, flags);
    applySeed(g, dim, seed);
    Range r = {scale, x, z, sx, sz, y, 1};
    int *cache = allocCache(g, r);
    if (!cache) { free(g); return -2; }
    int ret = genBiomes(g, cache, r);
    if (ret == 0) {
        int n = sx * sz;
        for (int i = 0; i < n; i++) {
            int id = cache[i];
            out[i] = (uint8_t)(id < 0 ? 255 : id);
        }
    }

    if (ret == 0 && out_shade && hs > 2) {
        float *h = (float *)malloc(sizeof(float) * hs * hs);
        if (h) {
            int useNoise = (mc >= MC_1_18 && dim == DIM_OVERWORLD &&
                            (g->bn.nptype == -1 || g->bn.nptype == NP_DEPTH));
            if (useNoise) {
                for (int j = 0; j < hs; j++) {
                    for (int i = 0; i < hs; i++) {
                        int64_t np[6];
                        sampleBiomeNoise(&g->bn, np, hx4 + i * hstep4, 0, hz4 + j * hstep4, 0, 0);
                        h[j * hs + i] = (float)(np[NP_DEPTH] / 76.0);
                    }
                }
            } else {
                // 旧版本 / 其他维度：用群系基准高度近似
                for (int j = 0; j < hs; j++) {
                    for (int i = 0; i < hs; i++) {
                        int ci = i * kstep;
                        int cj = j * kstep;
                        int id = (dim == DIM_OVERWORLD && cj < sz && ci < sx) ? cache[cj * sx + ci] : 0;
                        double d = 0, s = 0;
                        if (dim == DIM_OVERWORLD) getBiomeDepthAndScale(id, &d, &s, 0);
                        h[j * hs + i] = (float)(SHADE_UNIT + d * SHADE_PER_DEPTH);
                    }
                }
                if (dim == DIM_OVERWORLD) blur_field(h, hs);
            }
            compute_shade(h, hs, hstep4 * 4, out_shade);
            free(h);
        }
    }
    free(cache);
    free(g);
    return ret;
}
