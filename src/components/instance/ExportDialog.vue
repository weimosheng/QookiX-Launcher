<script setup lang="ts">
/**
 * 实例分享包导出对话框：勾选要打包的内容，选择保存位置后导出。
 */
import { ref } from "vue";
import { NButton, NCheckbox, NModal, useMessage } from "naive-ui";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../../api";

const props = defineProps<{ instanceId: string; instanceName: string }>();
const show = defineModel<boolean>("show", { required: true });

const message = useMessage();
const exporting = ref(false);

// 勾选项
const optMods = ref(true);
const optResourcepacks = ref(true);
const optShaders = ref(true);
const optWorlds = ref(false);
const optConfig = ref(false);

async function doExport() {
  const dest = await save({
    defaultPath: `${props.instanceName}.qkxinst`,
    filters: [{ name: "QookiX 实例分享包", extensions: ["qkxinst"] }],
  });
  if (!dest) return;
  exporting.value = true;
  try {
    const n = await api.exportInstancePack(props.instanceId, dest as string, {
      mods: optMods.value,
      resourcepacks: optResourcepacks.value,
      shaders: optShaders.value,
      worlds: optWorlds.value,
      config: optConfig.value,
    });
    message.success(`已导出 ${n} 个内容到 ${dest}`);
    show.value = false;
  } catch (e) {
    message.error(String(e));
  } finally {
    exporting.value = false;
  }
}
</script>

<template>
  <n-modal
    v-model:show="show"
    preset="card"
    title="导出实例分享包"
    style="width: 460px; max-width: 94vw"
    :mask-closable="true"
    :close-on-esc="true"
  >
    <div class="ex-body">
      <p class="ex-hint">
        在线安装的 mod 只会记录版本 ID（体积小），导入时自动从 Modrinth/CurseForge 重新下载；
        手动导入的文件会直接打包。游戏本体不会包含在包内，导入后自动安装。
      </p>
      <div class="ex-group">
        <label class="ex-check"><n-checkbox v-model:checked="optMods" /> 模组</label>
        <label class="ex-check"><n-checkbox v-model:checked="optResourcepacks" /> 资源包</label>
        <label class="ex-check"><n-checkbox v-model:checked="optShaders" /> 光影</label>
        <label class="ex-check">
          <n-checkbox v-model:checked="optWorlds" />
          世界存档
          <span class="ex-sub">（含每个世界的完整进度）</span>
        </label>
        <label class="ex-check">
          <n-checkbox v-model:checked="optConfig" />
          配置文件
          <span class="ex-sub">（config 目录 + options.txt）</span>
        </label>
      </div>
      <div class="ex-actions">
        <n-button @click="show = false">取消</n-button>
        <n-button type="primary" :loading="exporting" @click="doExport">选择位置并导出</n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.ex-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.ex-hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--text-3);
}
.ex-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ex-check {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-1);
  cursor: pointer;
}
.ex-sub {
  font-size: 11px;
  color: var(--text-3);
}
.ex-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 4px;
}
</style>
