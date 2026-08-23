<script setup lang="ts">
import { useLoadingBar } from "naive-ui";
import { setLoadingBarApi } from "../loadingBar";

setLoadingBarApi(useLoadingBar());
</script>

<template>
  <slot />
</template>
