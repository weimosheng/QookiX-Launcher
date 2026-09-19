<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { IconMapPin } from "../components/icons";
import seedCover from "../assets/seed-cover.png";

const { t } = useI18n();
const router = useRouter();

const tools = computed(() => [
  {
    key: "seed",
    title: t("toolbox.seedMapTitle"),
    desc: t("toolbox.seedMapDesc"),
    icon: IconMapPin,
    to: "/toolbox/seed",
    cover: "linear-gradient(135deg, #5aa2f0, #2f6bbd)",
    coverImg: seedCover,
  },
]);

function go(to: string) {
  router.push(to);
}
</script>

<template>
  <div id="toolbox-root" class="toolbox-view">
    <div class="grid">
      <section v-for="tool in tools" :key="tool.key" class="tool-card glass clickable" @click="go(tool.to)">
        <div class="tool-cover" :style="tool.coverImg ? { backgroundImage: `url(${tool.coverImg})` } : { background: tool.cover }">
          <component :is="tool.icon" class="tool-cover-icon" />
        </div>
        <div class="tool-body">
          <h3>{{ tool.title }}</h3>
          <p class="hint">{{ tool.desc }}</p>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.toolbox-view { display: flex; flex-direction: column; gap: 16px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 16px; }
.tool-card { overflow: hidden; display: flex; flex-direction: column; }
.tool-cover {
  height: 124px;
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
  background-size: cover;
  background-position: center;
}
.tool-cover-icon {
  font-size: 46px;
  color: rgba(255, 255, 255, 0.96);
  filter:
    drop-shadow(0 0 2px rgba(0, 0, 0, 0.85))
    drop-shadow(0 0 1px rgba(0, 0, 0, 0.85))
    drop-shadow(0 3px 12px rgba(0, 0, 0, 0.5));
}
.tool-body { padding: 16px 18px 18px; }
.tool-body h3 { margin: 0 0 10px; font-size: 15px; font-weight: 600; }
.hint { margin: 0; font-size: 12px; color: var(--text-3); line-height: 1.6; }
</style>
