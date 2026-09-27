/**
 * NBT 字段名的显示文本。
 *
 * 中文环境用 `nbt.field` 里的短名表（LevelName → 世界名称），
 * 未收录的键、以及英文环境（没有这份表）一律回落到原始键名，
 * 这样树形模式对小白可读，同时专业用户仍能对照 NBTExplorer。
 */
import { useI18n } from "vue-i18n";

export function useNbtLabels() {
  const { t, te } = useI18n();

  function fieldLabel(key: string): string {
    const path = `nbt.field.${key}`;
    return te(path) ? t(path) : key;
  }

  return { fieldLabel };
}
